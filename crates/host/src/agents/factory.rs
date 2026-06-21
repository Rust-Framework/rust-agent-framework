//! Agent factory — create built-in agents from configuration presets.

use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{anyhow, Result};
use tracing::warn;
use rust_agent_core::{IAgent, ModelMetadata};
use rust_agent_client::{ChatClientOptions, OpenAiChatClient};
use rust_agent_framework::{AgentBuilder, TokenBudgetStrategy, EstimateCounter};
use rust_agent_workflow::WorkflowAgent;

use crate::config::HostConfig;

/// 工厂，用于创建内置 Agent 实例。
pub struct AgentFactory<'a> {
    config: &'a HostConfig,
}

impl<'a> AgentFactory<'a> {
    pub fn new(config: &'a HostConfig) -> Self {
        Self { config }
    }

    /// 创建所有已启用的内置 Agent。
    /// 构造失败的 Agent 会被跳过并发出警告。
    pub async fn create_all(&self) -> Result<Vec<Arc<dyn IAgent>>> {
        let mut agents = Vec::new();

        if self.config.agents.coding {
            match self.create_coding_agent() {
                Ok(agent) => agents.push(agent),
                Err(e) => warn!(error = %e, "Failed to create coding agent, skipping"),
            }
        }
        if self.config.agents.general {
            match self.create_general_agent() {
                Ok(agent) => agents.push(agent),
                Err(e) => warn!(error = %e, "Failed to create general agent, skipping"),
            }
        }
        if self.config.agents.analysis {
            match self.create_analysis_agent() {
                Ok(agent) => agents.push(agent),
                Err(e) => warn!(error = %e, "Failed to create analysis agent, skipping"),
            }
        }

        if agents.is_empty() {
            warn!("No agents were created. Ensure AGNES_API_KEY (or OPENAI_API_KEY) is set and the provider config is correct.");
        }

        Ok(agents)
    }

    /// 创建开发流水线 Agent（6 阶段编码工作流，支持 HITL）。
    ///
    /// 返回 `(agent, graph)` 元组：
    /// - `agent`: `WorkflowAgent` 包装，用于注册到 `AgentRegistry`（提供元数据和子代理发现）
    /// - `graph`: 原始 `WorkflowGraph`，用于注册到 `WorkflowGraphRegistry`（HITL 执行路径）
    pub fn create_dev_pipeline_agent(&self) -> Result<(Arc<dyn IAgent>, rust_agent_workflow::WorkflowGraph)> {
        let options = self.build_client_options()?;
        let workspace_root = PathBuf::from(&self.config.workspace_root);

        let graph = rust_agent_coding::build_dev_pipeline(&options, &workspace_root)?;

        // Build a WorkflowAgent wrapper for metadata + sub-agent discovery.
        // The actual HITL execution uses the graph directly via WorkflowRuntime.
        let agent_id = self.config.dev_pipeline.agent_id.clone();
        let agent = Self::build_workflow_agent_with_id(graph.clone(), &agent_id)?;

        Ok((agent, graph))
    }

    /// Build a `WorkflowAgent` with a custom agent ID (overriding the default).
    fn build_workflow_agent_with_id(
        graph: rust_agent_workflow::WorkflowGraph,
        agent_id: &str,
    ) -> Result<Arc<dyn IAgent>> {
        let agent = WorkflowAgent::with_id(graph, agent_id);
        Ok(Arc::new(agent))
    }

    /// Build `ChatClientOptions` from the provider config.
    ///
    /// 装配所有配置字段：model、api_base、temperature、max_tokens、model_metadata。
    /// `model_metadata` 用于启用自动上下文压缩（TokenBudgetStrategy）。
    fn build_client_options(&self) -> Result<ChatClientOptions> {
        let provider = &self.config.provider;
        let api_key = provider
            .resolve_api_key()
            .ok_or_else(|| anyhow!(
                "No API key configured. Set AGNES_API_KEY or OPENAI_API_KEY environment variable, \
                 or configure via CLI: --api-key YOUR_KEY"
            ))?;

        let model = provider.model.clone();

        let mut options = match provider.provider.as_str() {
            "deepseek" => ChatClientOptions::deepseek(&model, api_key),
            _ => ChatClientOptions::openai(&model, api_key),
        };

        if let Some(url) = &provider.base_url {
            options.api_base = url.clone();
        }
        // 装配默认 temperature / max_tokens（可被每轮 AgentRunOptions 覆盖）
        options.temperature = provider.temperature;
        options.max_tokens = provider.max_tokens;

        // 装配 model_metadata — 启用自动上下文压缩的前提条件
        options.model_metadata = Some(ModelMetadata::new(
            model,
            provider.context_window_tokens,
            provider.max_output_tokens,
        ));

        Ok(options)
    }

    /// Create a chat client from the provider config.
    fn create_client(&self, model_override: Option<&str>) -> Result<OpenAiChatClient> {
        let provider = &self.config.provider;
        let api_key = provider
            .resolve_api_key()
            .ok_or_else(|| anyhow!(
                "No API key configured. Set AGNES_API_KEY or OPENAI_API_KEY environment variable, \
                 or configure via CLI: --api-key YOUR_KEY"
            ))?;

        let model = model_override.unwrap_or(&provider.model).to_string();

        let mut options = match provider.provider.as_str() {
            "deepseek" => ChatClientOptions::deepseek(&model, api_key),
            _ => ChatClientOptions::openai(&model, api_key),
        };

        if let Some(url) = &provider.base_url {
            options.api_base = url.clone();
        }
        options.temperature = provider.temperature;
        options.max_tokens = provider.max_tokens;
        options.model_metadata = Some(ModelMetadata::new(
            model,
            provider.context_window_tokens,
            provider.max_output_tokens,
        ));

        Ok(OpenAiChatClient::new(options)?)
    }

    /// 为 AgentBuilder 装配上下文压缩管线。
    ///
    /// 启用条件：provider 配置了 `context_window_tokens` 和 `max_output_tokens`。
    /// 使用 `TokenBudgetStrategy` + `EstimateCounter` 组合：
    /// - 超过输入预算时自动截断最早的非系统消息
    /// - 工具调用结果组被折叠为摘要
    fn apply_compression<C: rust_agent_core::IChatClient + 'static>(
        builder: AgentBuilder<C>,
    ) -> AgentBuilder<C> {
        builder
            .with_compression_strategy(Arc::new(TokenBudgetStrategy::new()))
            .with_token_counter(Arc::new(EstimateCounter::new()))
    }

    /// Create the coding agent.
    fn create_coding_agent(&self) -> Result<Arc<dyn IAgent>> {
        let client = self.create_client(None)?;

        let agent = Self::apply_compression(AgentBuilder::new("coding"))
            .chat_client(client)
            .instructions(
                "你是资深软件工程师，专注于代码生成、代码审查、Bug 定位和代码重构。\n\n\
                工作原则：\n\
                1. 先理解需求，再动手写代码\n\
                2. 代码风格清晰，注释精炼\n\
                3. 优先使用标准库，避免不必要的依赖\n\
                4. 代码块使用 markdown 格式并标注语言\n\
                5. 修改文件前先读取文件内容\n\
                6. 用中文回复")
            .with_description("代码专家智能体 — 代码生成、审查、调试、重构")
            .with_tool(rust_agent_framework::tools::ReadFile::default())
            .with_tool(rust_agent_framework::tools::WriteFile::default())
            .with_tool(rust_agent_framework::tools::EditFile::default())
            .with_tool(rust_agent_framework::tools::ListFiles::default())
            .with_tool(rust_agent_framework::tools::SearchFile::default())
            .with_tool(rust_agent_framework::tools::FindFiles::default())
            .with_tool(rust_agent_framework::tools::RunCommand::default())
            .with_tool(rust_agent_framework::tools::MakeDirectory::default())
            .with_tool(rust_agent_framework::tools::MoveFile::default())
            .with_tool(rust_agent_framework::tools::RemovePath::default())
            .with_tool(rust_agent_framework::tools::InspectFile::default())
            .max_tool_rounds(15)
            .build()?;

        Ok(agent)
    }

    /// Create the general agent.
    fn create_general_agent(&self) -> Result<Arc<dyn IAgent>> {
        let client = self.create_client(None)?;

        let agent = Self::apply_compression(AgentBuilder::new("general"))
            .chat_client(client)
            .instructions(
                "你是通用 AI 助手，擅长回答问题、写作、分析和创意工作。\n\n\
                回复原则：\n\
                1. 先给出直接答案，再展开解释\n\
                2. 遇到不确定的问题，明确说明不确定性\n\
                3. 使用 markdown 格式组织长回复\n\
                4. 用中文回复")
            .with_description("通用 AI 助手 — 回答问题、写作、分析、创意")
            .max_tool_rounds(5)
            .build()?;

        Ok(agent)
    }

    /// Create the analysis agent.
    fn create_analysis_agent(&self) -> Result<Arc<dyn IAgent>> {
        let client = self.create_client(None)?;

        let agent = Self::apply_compression(AgentBuilder::new("analysis"))
            .chat_client(client)
            .instructions(
                "你是数据分析师，专注于深度研究、多源对比和趋势分析。\n\n\
                工作方法：\n\
                1. 先定义分析框架，再逐点展开\n\
                2. 引用具体数据和来源\n\
                3. 给出可操作的结论和建议\n\
                4. 使用表格和列表增强可读性\n\
                5. 用中文回复")
            .with_description("数据分析师 — 深度研究、多源对比、趋势分析")
            .with_tool(rust_agent_framework::tools::ReadFile::default())
            .max_tool_rounds(10)
            .build()?;

        Ok(agent)
    }
}
