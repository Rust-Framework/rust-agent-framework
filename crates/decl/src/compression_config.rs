use std::sync::Arc;

use rust_agent_core::{ICompressionStrategy, ITokenCounter};
use rust_agent_framework::{EstimateCounter, SlidingWindowStrategy, TokenBudgetStrategy};
use serde::{Deserialize, Serialize};

/// 声明式压缩策略（框架扩展，非 MAF 核心字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompressionDecl {
    /// 滑动窗口：保留最近 N 条非系统消息。
    SlidingWindow {
        #[serde(rename = "windowSize")]
        window_size: usize,
    },
    /// Token 预算：超出模型上下文预算时裁剪最早消息。
    TokenBudget {
        #[serde(default, rename = "toolResultEvictionThreshold")]
        tool_result_eviction_threshold: Option<f64>,
    },
}

/// 声明式 Token 计数器（框架扩展）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TokenCounterDecl {
    /// 基于字符长度的近似估算（默认）。
    Estimate,
}

pub fn build_compression_strategy(decl: &CompressionDecl) -> Arc<dyn ICompressionStrategy> {
    match decl {
        CompressionDecl::SlidingWindow { window_size } => {
            Arc::new(SlidingWindowStrategy::new(*window_size))
        }
        CompressionDecl::TokenBudget {
            tool_result_eviction_threshold,
        } => {
            let mut strategy = TokenBudgetStrategy::new();
            if let Some(threshold) = tool_result_eviction_threshold {
                strategy = strategy.with_eviction_threshold(*threshold);
            }
            Arc::new(strategy)
        }
    }
}

pub fn build_token_counter(_decl: &TokenCounterDecl) -> Arc<dyn ITokenCounter> {
    Arc::new(EstimateCounter::new())
}
