use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use llama_gguf::engine::{Engine, EngineConfig, EngineError};
use rust_agent_core::{
    AgentError, AgentResponseUpdate, ChatClientRunOptions, ChatMessage, FinishReason, Result,
    Usage,
};
use tokio::sync::mpsc;
use tracing::warn;

use crate::options::LlamaChatClientOptions;
use crate::prompt::{build_prompt, detect_prompt_style, find_stop};

pub struct LlamaEngine {
    engine: Engine,
    default_temperature: f32,
    default_top_p: f32,
    default_max_tokens: u32,
    model_id: String,
    context_window: u32,
}

impl LlamaEngine {
    pub fn load(options: &LlamaChatClientOptions) -> Result<Self> {
        let mut config = EngineConfig {
            model_path: options.model_path.clone(),
            tokenizer_path: options.tokenizer_path.clone(),
            temperature: options.temperature.unwrap_or(0.7),
            top_p: options.top_p.unwrap_or(0.9),
            max_tokens: options.max_tokens.unwrap_or(512) as usize,
            seed: options.seed,
            use_gpu: options.use_gpu.unwrap_or(false),
            max_context_len: options.max_context_len,
            ..Default::default()
        };

        if config.seed.is_none() {
            config.seed = Some(random_seed());
        }

        let engine = Engine::load(config).map_err(map_engine_error)?;

        let context_window = engine.model_config().max_seq_len as u32;

        Ok(Self {
            engine,
            default_temperature: options.temperature.unwrap_or(0.7),
            default_top_p: options.top_p.unwrap_or(0.9),
            default_max_tokens: options.max_tokens.unwrap_or(512),
            model_id: options.model_id.clone(),
            context_window,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn context_window(&self) -> u32 {
        self.context_window
    }

    pub fn generate(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
        tx: mpsc::Sender<Result<AgentResponseUpdate>>,
    ) -> Result<()> {
        if !run_options.tools.is_empty() {
            warn!("LlamaChatClient: tool definitions are ignored for local inference");
        }

        let prompt_style = detect_prompt_style(&self.engine);
        let prompt = build_prompt(messages, &prompt_style);
        if prompt.trim().is_empty() {
            return Err(AgentError::ChatClientError(
                "empty prompt after message conversion".into(),
            ));
        }

        let _ = tx.blocking_send(Ok(AgentResponseUpdate::ResponseMetadata {
            id: None,
            model: Some(self.model_id.clone()),
        }));

        let max_tokens = run_options
            .max_tokens
            .unwrap_or(self.default_max_tokens) as usize;
        let _temperature = run_options
            .temperature
            .unwrap_or(self.default_temperature);
        let _top_p = run_options.top_p.unwrap_or(self.default_top_p);
        let cancelled = run_options.cancelled.clone();

        // Per-call temperature/top_p overrides require EngineConfig at load time;
        // run_options overrides apply to max_tokens only for now.
        let stream = self.engine.generate_streaming(&prompt, max_tokens);

        let prompt_tokens = self.estimate_prompt_tokens(&prompt);
        let mut completion_tokens: u32 = 0;
        let mut generated = String::new();

        for chunk in stream {
            if is_cancelled(&cancelled) {
                send_finish(
                    &tx,
                    FinishReason::Other("cancelled".into()),
                    prompt_tokens,
                    completion_tokens,
                );
                return Ok(());
            }

            match chunk {
                Ok(delta) => {
                    if delta.is_empty() {
                        continue;
                    }

                    let combined = format!("{generated}{delta}");
                    if let Some(stop_at) =
                        find_stop(&combined, &prompt_style, self.engine.chat_template())
                    {
                        let tail = &combined[generated.len()..stop_at];
                        if !tail.is_empty() {
                            completion_tokens += 1;
                            let _ = tx.blocking_send(Ok(AgentResponseUpdate::TextDelta {
                                delta: tail.to_string(),
                            }));
                        }
                        send_finish(
                            &tx,
                            FinishReason::Stop,
                            prompt_tokens,
                            completion_tokens,
                        );
                        return Ok(());
                    }

                    completion_tokens += 1;
                    generated.push_str(&delta);
                    let _ = tx.blocking_send(Ok(AgentResponseUpdate::TextDelta { delta }));
                }
                Err(e) => {
                    return Err(map_engine_error(e));
                }
            }
        }

        send_finish(
            &tx,
            FinishReason::Stop,
            prompt_tokens,
            completion_tokens,
        );
        Ok(())
    }

    fn estimate_prompt_tokens(&self, prompt: &str) -> usize {
        self.engine
            .tokenizer()
            .encode(prompt, self.engine.add_bos())
            .map(|tokens| tokens.len())
            .unwrap_or_else(|_| prompt.len() / 4)
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

fn map_engine_error(err: EngineError) -> AgentError {
    AgentError::ChatClientError(err.to_string())
}
