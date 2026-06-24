use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, ModelMetadata,
    Result,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::engine::LlamaEngine;
use crate::options::LlamaChatClientOptions;

/// 基于 [llama-gguf](https://crates.io/crates/llama-gguf) 的本地 GGUF 推理 `IChatClient`。
pub struct LlamaChatClient {
    engine: Arc<LlamaEngine>,
    model_metadata: Option<ModelMetadata>,
}

impl LlamaChatClient {
    pub fn new(options: LlamaChatClientOptions) -> Result<Self> {
        let engine = LlamaEngine::load(&options)?;
        let model_metadata = options.model_metadata.clone().or_else(|| {
            Some(default_model_metadata(
                &options.model_id,
                engine.context_window(),
            ))
        });
        Ok(Self {
            engine: Arc::new(engine),
            model_metadata,
        })
    }
}

#[async_trait]
impl IChatClient for LlamaChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let (tx, rx) = mpsc::channel::<Result<AgentResponseUpdate>>(64);
        let engine = Arc::clone(&self.engine);
        let messages = messages.to_vec();

        tokio::task::spawn_blocking(move || {
            if let Err(e) = engine.generate(&messages, &options, tx.clone()) {
                let _ = tx.blocking_send(Err(e));
            }
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn model_id(&self) -> &str {
        self.engine.model_id()
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.model_metadata.as_ref()
    }
}

/// 按模型 ID 与实际上下文长度返回参考元数据。
pub fn default_model_metadata(model_id: &str, context_window: u32) -> ModelMetadata {
    let lower = model_id.to_lowercase();
    let max_output = if lower.contains("70b") || lower.contains("32b") {
        2048
    } else if lower.contains("8b") || lower.contains("7b") {
        2048
    } else if lower.contains("3b") {
        1024
    } else {
        512
    };
    ModelMetadata::new(model_id, context_window as usize, max_output as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metadata_uses_context_window() {
        let meta = default_model_metadata("llama-3.2-1b-it", 8192);
        assert_eq!(meta.context_window_tokens, 8192);
        assert!(meta.max_output_tokens > 0);
    }
}
