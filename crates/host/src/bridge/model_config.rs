//! 每轮模型配置（Per-Turn Model Configuration）。
//!
//! IDE 客户端可以在每轮 `session/prompt` 请求中通过 `_meta.raf.model_config`
//! 传递模型配置覆盖项，包括：
//!
//! - `model_id`：模型 ID 覆盖（仅影响日志/元数据，不切换已绑定的客户端）
//! - `temperature`：温度参数覆盖
//! - `max_tokens`：最大输出 token 数覆盖
//! - `thinking`：是否启用思考（推理）模式
//! - `thinking_level`：思考等级（"high" 或 "max"）
//! - `context_window_tokens`：上下文窗口大小覆盖（影响压缩预算）
//! - `max_output_tokens`：最大输出 token 数覆盖（影响压缩预算）
//!
//! 所有字段均为 `Option`——未指定的字段回退到 Agent/客户端的默认值。
//!
//! ## 协议约定
//!
//! ACP 协议本身没有原生的模型配置字段。RAF 使用 `_meta.raf.model_config`
//! 命名空间作为私有扩展通道。客户端应遵循此约定传递每轮配置。
//!
//! ```json
//! {
//!   "method": "session/prompt",
//!   "params": {
//!     "session_id": "...",
//!     "prompt": [...],
//!     "_meta": {
//!       "raf": {
//!         "agent_id": "coding",
//!         "model_config": {
//!           "temperature": 0.3,
//!           "max_tokens": 4096,
//!           "thinking": true,
//!           "thinking_level": "high"
//!         }
//!       }
//!     }
//!   }
//! }
//! ```

use tracing::debug;

use rust_agent_core::{AgentRunOptions, ReasoningEffort};

/// 每轮模型配置覆盖项。
///
/// 从 `session/prompt` 请求的 `_meta.raf.model_config` 中解析。
/// 所有字段为 `Option`，`None` 表示使用默认值。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PerTurnModelConfig {
    /// 模型 ID 覆盖（仅用于元数据记录，不切换已绑定的 LLM 客户端）。
    #[serde(default)]
    pub model_id: Option<String>,

    /// 温度参数覆盖（0.0 - 2.0）。
    #[serde(default)]
    pub temperature: Option<f32>,

    /// 最大输出 token 数覆盖。
    #[serde(default)]
    pub max_tokens: Option<u32>,

    /// 是否启用思考（推理）模式。默认 `true`（如果未指定则不覆盖）。
    #[serde(default)]
    pub thinking: Option<bool>,

    /// 思考等级："high" 或 "max"。仅在 `thinking: true` 时生效。
    #[serde(default)]
    pub thinking_level: Option<String>,

    /// 上下文窗口大小覆盖（token 数）。影响自动压缩的预算计算。
    #[serde(default)]
    pub context_window_tokens: Option<usize>,

    /// 最大输出 token 数覆盖（影响压缩预算）。
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
}

impl PerTurnModelConfig {
    /// 从 ACP `_meta` 中解析每轮模型配置。
    ///
    /// 查找路径：`_meta.raf.model_config`
    ///
    /// ACP 的 `_meta` 字段类型为 `serde_json::Map<String, Value>`（即 `Meta` 类型别名）。
    pub fn from_meta(meta: Option<&serde_json::Map<String, serde_json::Value>>) -> Self {
        let Some(meta) = meta else {
            return Self::default();
        };

        let config_value = meta
            .get("raf")
            .and_then(|raf| raf.get("model_config"));

        let Some(config_value) = config_value else {
            return Self::default();
        };

        match serde_json::from_value::<PerTurnModelConfig>(config_value.clone()) {
            Ok(config) => {
                debug!(?config, "Parsed per-turn model config from _meta");
                config
            }
            Err(e) => {
                debug!(error = %e, "Failed to parse per-turn model config, using defaults");
                Self::default()
            }
        }
    }

    /// 将每轮配置应用到 `AgentRunOptions`。
    ///
    /// - `temperature` / `max_tokens` 直接映射
    /// - `thinking` 映射到 `extra_body.thinking`
    /// - `thinking_level` 映射到 `extra_body.reasoning_effort`
    ///
    /// `context_window_tokens` / `max_output_tokens` 不在此处应用——
    /// 它们影响的是 Agent 的压缩预算，而非单次运行的 LLM 参数。
    pub fn apply_to_run_options(&self, mut opts: AgentRunOptions) -> AgentRunOptions {
        if let Some(temp) = self.temperature {
            opts = opts.with_temperature(temp);
        }
        if let Some(max_tok) = self.max_tokens {
            opts = opts.with_max_tokens(max_tok);
        }
        if let Some(thinking) = self.thinking {
            opts = opts.with_thinking(thinking);
        }
        if let Some(ref level) = self.thinking_level {
            match level.as_str() {
                "high" => {
                    opts = opts.with_reasoning_effort(ReasoningEffort::High);
                }
                "max" => {
                    opts = opts.with_reasoning_effort(ReasoningEffort::Max);
                }
                _ => {
                    debug!(level = %level, "Unknown thinking_level, ignoring (expected 'high' or 'max')");
                }
            }
        }
        opts
    }

    /// 是否包含任何非空配置。
    pub fn is_empty(&self) -> bool {
        self.model_id.is_none()
            && self.temperature.is_none()
            && self.max_tokens.is_none()
            && self.thinking.is_none()
            && self.thinking_level.is_none()
            && self.context_window_tokens.is_none()
            && self.max_output_tokens.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_meta_empty() {
        let config = PerTurnModelConfig::from_meta(None);
        assert!(config.is_empty());
    }

    #[test]
    fn test_from_meta_full() {
        let mut meta = serde_json::Map::new();
        meta.insert("raf".to_string(), serde_json::json!({
            "model_config": {
                "temperature": 0.5,
                "max_tokens": 2048,
                "thinking": false,
                "thinking_level": "max"
            }
        }));
        let config = PerTurnModelConfig::from_meta(Some(&meta));
        assert_eq!(config.temperature, Some(0.5));
        assert_eq!(config.max_tokens, Some(2048));
        assert_eq!(config.thinking, Some(false));
        assert_eq!(config.thinking_level.as_deref(), Some("max"));
    }

    #[test]
    fn test_apply_to_run_options() {
        let config = PerTurnModelConfig {
            temperature: Some(0.3),
            max_tokens: Some(4096),
            thinking: Some(true),
            thinking_level: Some("high".to_string()),
            ..Default::default()
        };

        let opts = config.apply_to_run_options(AgentRunOptions::new());
        assert_eq!(opts.temperature, Some(0.3));
        assert_eq!(opts.max_tokens, Some(4096));
        assert!(opts.extra_body.contains_key("thinking"));
        assert!(opts.extra_body.contains_key("reasoning_effort"));
    }
}
