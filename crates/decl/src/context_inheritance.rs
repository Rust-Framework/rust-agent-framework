//! 子 Agent 继承父 Agent 的声明式 contexts（workspace / memory / skills 等）。

use crate::context_provider_config::ContextProviderDecl;
use crate::definition::{AgentDefinition, AgentKindData};
use crate::prompt_agent::PromptAgentData;

/// 将父 Agent 的 contexts 合并到子 Agent（同 kind 不重复，子 Agent 已有则保留子配置）。
pub fn inherit_parent_contexts(sub: &mut AgentDefinition, parent: &PromptAgentData) {
    let sub_data = match &mut sub.kind_data {
        AgentKindData::Prompt(data) => data,
        _ => return,
    };

    for parent_ctx in &parent.contexts {
        let parent_kind = parent_ctx.kind_str();
        let parent_name = parent_ctx.name();
        let already = sub_data.contexts.iter().any(|c| {
            c.kind_str() == parent_kind && c.name() == parent_name
        });
        if !already {
            sub_data.contexts.push(parent_ctx.clone());
        }
    }
}

/// 父 Agent 是否声明了 workspace context。
pub fn parent_has_workspace(parent: &PromptAgentData) -> bool {
    parent
        .contexts
        .iter()
        .any(|c| matches!(c, ContextProviderDecl::Workspace { .. }))
}
