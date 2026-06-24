use std::sync::Arc;

use async_trait::async_trait;
use lmrs::transformer::ModelType;
use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, ModelMetadata,
    Result,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::engine::LmEngine;
use crate::options::LmChatClientOptions;

/// 基于 [lm.rs](https://github.com/samuel-vitorino/lm.rs) 的本地 CPU 推理 `IChatClient`。
///
/// 支持 Gemma 2、Llama 3.2、Phi-3.5 等 LMRS 格式量化模型。
pub struct LmChatClient {
    engine: Arc<LmEngine>,
    model_metadata: Option<ModelMetadata>,
}

impl LmChatClient {
    pub fn new(options: LmChatClientOptions) -> Result<Self> {
        let model_metadata = options
            .model_metadata
            .clone()
            .or_else(|| Some(default_model_metadata(&options.model_id)));
        let engine = LmEngine::load(&options)?;
        Ok(Self {
            engine: Arc::new(engine),
            model_metadata,
        })
    }

    pub fn engine(&self) -> &Arc<LmEngine> {
        &self.engine
    }
}

#[async_trait]
impl IChatClient for LmChatClient {
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

        let stream = ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    fn model_id(&self) -> &str {
        self.engine.model_id()
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.model_metadata.as_ref()
    }
}

/// 按常见 LMRS 模型 ID 返回参考元数据。
pub fn default_model_metadata(model_id: &str) -> ModelMetadata {
    let lower = model_id.to_lowercase();
    let (context, max_output) = if lower.contains("9b") {
        (8192, 2048)
    } else if lower.contains("3b") {
        (8192, 2048)
    } else if lower.contains("2b") {
        (8192, 2048)
    } else if lower.contains("1b") {
        (8192, 1024)
    } else if lower.contains("phi") {
        (4096, 1024)
    } else {
        (4096, 512)
    };
    ModelMetadata::new(model_id, context, max_output)
}

/// 从已加载引擎推断模型族名称。
pub fn model_family_name(model_type: ModelType) -> &'static str {
    match model_type {
        ModelType::GEMMA => "gemma",
        ModelType::LLAMA => "llama",
        ModelType::PHI => "phi",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metadata_is_reasonable() {
        let meta = default_model_metadata("llama-3.2-1b-it");
        assert!(meta.context_window_tokens >= 4096);
        assert!(meta.max_output_tokens > 0);
    }
}
