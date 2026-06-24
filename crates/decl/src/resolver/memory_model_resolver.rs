//! Resolves optional `memoryModel` config for Super Brain curator sub-agents.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::DeclError;
use crate::Result;
use rust_agent_core::IChatClient;
#[cfg(feature = "llama")]
use rust_agent_framework::super_brain::wrap_super_brain_curator_client;

/// Build a dedicated chat client for Super Brain memory consolidation.
///
/// Reads `config.memoryModel` (or `memory_model`) from the super-brain context block.
pub fn resolve_super_brain_memory_client(
    config: &HashMap<String, serde_json::Value>,
) -> Result<Option<Arc<dyn IChatClient>>> {
    let Some(model) = config
        .get("memoryModel")
        .or_else(|| config.get("memory_model"))
    else {
        return Ok(None);
    };

    let provider = model
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("llama")
        .to_lowercase();

    match provider.as_str() {
        "llama" | "gguf" | "local" | "lm" => resolve_llama_memory_client(model),
        other => Err(DeclError::Unsupported(format!(
            "unsupported super-brain memoryModel provider '{other}' (supported: llama, gguf)"
        ))),
    }
}

#[cfg(feature = "llama")]
fn resolve_llama_memory_client(
    model: &serde_json::Value,
) -> Result<Option<Arc<dyn IChatClient>>> {
    use rust_agent_llama::{LlamaChatClient, LlamaChatClientOptions};

    let model_path = model
        .get("modelPath")
        .or_else(|| model.get("model_path"))
        .or_else(|| model.get("path"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            DeclError::Missing(
                "super-brain memoryModel requires modelPath (GGUF file path)".into(),
            )
        })?;

    let model_id = model
        .get("id")
        .or_else(|| model.get("model_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("super-brain-memory");

    let mut options = LlamaChatClientOptions::new(model_path, model_id);

    if let Some(path) = model
        .get("tokenizerPath")
        .or_else(|| model.get("tokenizer_path"))
        .and_then(|v| v.as_str())
    {
        options = options.with_tokenizer_path(path);
    }
    if let Some(temp) = model.get("temperature").and_then(|v| v.as_f64()) {
        options = options.with_temperature(temp as f32);
    }
    if let Some(top_p) = model.get("topP").or_else(|| model.get("top_p")).and_then(|v| v.as_f64())
    {
        options = options.with_top_p(top_p as f32);
    }
    if let Some(max_tokens) = model
        .get("maxTokens")
        .or_else(|| model.get("max_tokens"))
        .and_then(|v| v.as_u64())
    {
        options = options.with_max_tokens(max_tokens as u32);
    }
    if let Some(seed) = model.get("seed").and_then(|v| v.as_u64()) {
        options = options.with_seed(seed);
    }
    if let Some(use_gpu) = model.get("useGpu").or_else(|| model.get("use_gpu")).and_then(|v| v.as_bool()) {
        options = options.with_use_gpu(use_gpu);
    }

    let client = LlamaChatClient::new(options).map_err(|e| {
        let hint = if model_path.to_lowercase().contains("granite")
            && (model_path.contains("-h-") || model_path.contains("hybrid"))
        {
            " (IBM Granite Hybrid / SSM models are not yet supported by llama-gguf; use a standard LLaMA/Qwen/Gemma GGUF, e.g. gemma-3-270m-it)"
        } else {
            ""
        };
        DeclError::Agent(rust_agent_core::AgentError::ChatClientError(format!(
            "{e}{hint}"
        )))
    })?;
    Ok(Some(wrap_super_brain_curator_client(Arc::new(client))))
}

#[cfg(not(feature = "llama"))]
fn resolve_llama_memory_client(
    _model: &serde_json::Value,
) -> Result<Option<Arc<dyn IChatClient>>> {
    Err(DeclError::Unsupported(
        "super-brain memoryModel with llama provider requires rust-agent-decl `llama` feature"
            .into(),
    ))
}
