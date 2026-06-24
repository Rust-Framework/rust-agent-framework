//! Utilities for unwrapping decorator chains and cloning leaf clients.

use std::sync::Arc;

use rust_agent_core::IChatClient;

use crate::agnes_client::AgnesChatClient;
use crate::anthropic_client::AnthropicChatClient;
use crate::chat_client::ChatClient;
use crate::deepseek_client::DeepSeekChatClient;
use crate::openai_client::OpenAiChatClient;
use crate::options::ChatClientOptions;

/// Walk `inner_client()` links to the leaf API client.
pub fn unwrap_chat_client_leaf(client: &Arc<dyn IChatClient>) -> Arc<dyn IChatClient> {
    let mut current = Arc::clone(client);
    loop {
        if let Some(inner) = current.inner_client() {
            current = Arc::clone(inner);
        } else {
            return current;
        }
    }
}

/// Curator consolidation timeout (seconds). Override via `RAF_CURATOR_TIMEOUT_SECS`.
pub fn curator_timeout_secs() -> u64 {
    std::env::var("RAF_CURATOR_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180)
}

/// Clone a leaf client with a longer HTTP timeout for background bundle curation.
pub fn clone_leaf_with_timeout(
    leaf: Arc<dyn IChatClient>,
    timeout_secs: u64,
) -> Arc<dyn IChatClient> {
    macro_rules! try_rebuild {
        ($ty:ty, $extract:expr, $build:expr) => {
            if let Ok(typed) = Arc::downcast::<$ty>(leaf.clone()) {
                let mut opts: ChatClientOptions = $extract(&typed);
                opts.timeout_secs = Some(timeout_secs);
                if let Ok(client) = $build(opts) {
                    return Arc::new(client);
                }
            }
        };
    }

    try_rebuild!(
        AgnesChatClient,
        |c: &AgnesChatClient| c.inner().options().clone(),
        AgnesChatClient::new
    );
    try_rebuild!(
        OpenAiChatClient,
        |c: &OpenAiChatClient| c.inner().options().clone(),
        OpenAiChatClient::new
    );
    try_rebuild!(
        DeepSeekChatClient,
        |c: &DeepSeekChatClient| c.inner().options().clone(),
        DeepSeekChatClient::new
    );
    try_rebuild!(
        AnthropicChatClient,
        |c: &AnthropicChatClient| c.options().clone(),
        AnthropicChatClient::new
    );
    try_rebuild!(
        ChatClient,
        |c: &ChatClient| c.options().clone(),
        ChatClient::new
    );

    leaf
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rust_agent_core::{
        AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, Result,
    };

    struct OuterClient {
        inner: Arc<dyn IChatClient>,
    }

    #[async_trait]
    impl IChatClient for OuterClient {
        async fn run(
            &self,
            messages: &[ChatMessage],
            options: ChatClientRunOptions,
        ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
            self.inner.run(messages, options).await
        }

        fn model_id(&self) -> &str {
            self.inner.model_id()
        }

        fn inner_client(&self) -> Option<&Arc<dyn IChatClient>> {
            Some(&self.inner)
        }
    }

    #[test]
    fn unwrap_reaches_leaf_through_decorators() {
        let leaf = Arc::new(
            ChatClient::new(ChatClientOptions::openai("gpt-4", "sk-test")).unwrap(),
        ) as Arc<dyn IChatClient>;
        let wrapped = Arc::new(OuterClient {
            inner: Arc::new(ChatClientWrapperLike(leaf.clone())),
        }) as Arc<dyn IChatClient>;

        let unwrapped = unwrap_chat_client_leaf(&wrapped);
        assert_eq!(unwrapped.model_id(), leaf.model_id());
    }

    struct ChatClientWrapperLike(Arc<dyn IChatClient>);

    #[async_trait]
    impl IChatClient for ChatClientWrapperLike {
        async fn run(
            &self,
            messages: &[ChatMessage],
            options: ChatClientRunOptions,
        ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
            self.0.run(messages, options).await
        }

        fn model_id(&self) -> &str {
            self.0.model_id()
        }

        fn inner_client(&self) -> Option<&Arc<dyn IChatClient>> {
            Some(&self.0)
        }
    }
}
