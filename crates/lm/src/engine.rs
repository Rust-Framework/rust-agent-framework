use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use lmrs::sampler::Sampler;
use lmrs::tokenizer::Tokenizer;
use lmrs::transformer::{ModelType, Transformer};
use memmap2::Mmap;
use parking_lot::Mutex;
use rust_agent_core::{
    AgentError, AgentResponseUpdate, ChatClientRunOptions, ChatMessage, FinishReason, Result,
    Usage,
};
use tokio::sync::mpsc;
use tracing::warn;

use crate::options::LmChatClientOptions;
use crate::prompt::build_prompt_tokens;

/// 持有 mmap 与 tokenizer，每次推理时从 mmap 构造新的 `Transformer`（避免自引用生命周期问题）。
pub struct LmEngine {
    mmap: Mmap,
    tokenizer: Mutex<Tokenizer>,
    model_type: ModelType,
    vocab_size: u32,
    eos: u32,
    seq_len: u32,
    default_temperature: f32,
    default_top_p: f32,
    default_max_tokens: u32,
    default_seed: Option<u64>,
    model_id: String,
}

impl LmEngine {
    pub fn load(options: &LmChatClientOptions) -> Result<Self> {
        let file = File::open(&options.model_path).map_err(|e| {
            AgentError::ConfigError(format!(
                "failed to open model file '{}': {e}",
                options.model_path
            ))
        })?;
        let mmap = unsafe {
            Mmap::map(&file).map_err(|e| {
                AgentError::ConfigError(format!(
                    "failed to mmap model file '{}': {e}",
                    options.model_path
                ))
            })?
        };

        let (transformer, _) = Transformer::new(&mmap);
        let model_type = transformer.args.model_type;
        let vocab_size = transformer.args.vocab_size;
        // lm.rs caps effective context at 8192 in Transformer::new when seq_len > 8192.
        let seq_len = 8192u32;
        drop(transformer);

        let tokenizer = Tokenizer::new(&options.tokenizer_path);
        let eos = tokenizer.eos;

        Ok(Self {
            mmap,
            tokenizer: Mutex::new(tokenizer),
            model_type,
            vocab_size,
            eos,
            seq_len,
            default_temperature: options.temperature.unwrap_or(0.7),
            default_top_p: options.top_p.unwrap_or(0.9),
            default_max_tokens: options.max_tokens.unwrap_or(512),
            default_seed: options.seed,
            model_id: options.model_id.clone(),
        })
    }

    pub fn model_type(&self) -> ModelType {
        self.model_type
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn generate(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
        tx: mpsc::Sender<Result<AgentResponseUpdate>>,
    ) -> Result<()> {
        if !run_options.tools.is_empty() {
            warn!("LmChatClient: tool definitions are ignored for local inference");
        }

        let prompt_tokens = {
            let mut tokenizer = self.tokenizer.lock();
            build_prompt_tokens(&mut tokenizer, messages, self.model_type)
        };

        if prompt_tokens.is_empty() {
            return Err(AgentError::ChatClientError(
                "empty prompt after message conversion".into(),
            ));
        }

        if prompt_tokens.len() as u32 > self.seq_len {
            return Err(AgentError::ChatClientError(format!(
                "prompt length {} exceeds model seq_len {}",
                prompt_tokens.len(),
                self.seq_len
            )));
        }

        let _ = tx.blocking_send(Ok(AgentResponseUpdate::ResponseMetadata {
            id: None,
            model: Some(self.model_id.clone()),
        }));

        let (mut transformer, _) = Transformer::new(&self.mmap);

        let temperature = run_options
            .temperature
            .unwrap_or(self.default_temperature);
        let top_p = run_options.top_p.unwrap_or(self.default_top_p);
        let max_tokens = run_options
            .max_tokens
            .unwrap_or(self.default_max_tokens);
        let seed = self.default_seed.unwrap_or_else(random_seed);

        let mut sampler = Sampler::new(self.vocab_size, temperature, top_p, seed);
        let tokenizer = self.tokenizer.lock();

        let num_prompt_tokens = prompt_tokens.len();
        let mut pos: u32 = 0;
        let mut next: u32 = 0;
        let mut completion_tokens: u32 = 0;
        let cancelled = run_options.cancelled.clone();

        for (user_idx, &token) in prompt_tokens.iter().enumerate() {
            if is_cancelled(&cancelled) {
                send_finish(
                    &tx,
                    FinishReason::Other("cancelled".into()),
                    num_prompt_tokens,
                    completion_tokens,
                );
                return Ok(());
            }

            let logits = transformer.forward(token, pos);
            pos += 1;

            if user_idx + 1 == num_prompt_tokens {
                next = sampler.sample(logits);
            }
        }

        loop {
            if is_cancelled(&cancelled) {
                send_finish(
                    &tx,
                    FinishReason::Other("cancelled".into()),
                    num_prompt_tokens,
                    completion_tokens,
                );
                return Ok(());
            }

            if completion_tokens >= max_tokens {
                send_finish(&tx, FinishReason::Length, num_prompt_tokens, completion_tokens);
                return Ok(());
            }

            if next == self.eos || (self.model_type == ModelType::GEMMA && next == 107) {
                send_finish(&tx, FinishReason::Stop, num_prompt_tokens, completion_tokens);
                return Ok(());
            }

            let piece = tokenizer.decode(next);
            if !piece.is_empty() {
                let _ = tx.blocking_send(Ok(AgentResponseUpdate::TextDelta { delta: piece }));
            }

            completion_tokens += 1;

            let logits = transformer.forward(next, pos);
            next = sampler.sample(logits);
            pos += 1;
        }
    }
}

fn is_cancelled(cancelled: &Option<Arc<AtomicBool>>) -> bool {
    cancelled
        .as_ref()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn send_finish(
    tx: &mpsc::Sender<Result<AgentResponseUpdate>>,
    finish_reason: FinishReason,
    prompt_tokens: usize,
    completion_tokens: u32,
) {
    let usage = Usage {
        prompt_tokens: prompt_tokens as u32,
        completion_tokens,
        total_tokens: prompt_tokens as u32 + completion_tokens,
        ..Default::default()
    };
    let _ = tx.blocking_send(Ok(AgentResponseUpdate::Usage {
        usage: usage.clone(),
    }));
    let _ = tx.blocking_send(Ok(AgentResponseUpdate::Finish {
        finish_reason,
        usage: Some(usage),
    }));
}
