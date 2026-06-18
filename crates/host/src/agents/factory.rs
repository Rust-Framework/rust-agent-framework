//! Agent factory — create built-in agents from configuration presets.

use std::sync::Arc;
use anyhow::{anyhow, Result};
use tracing::warn;
use rust_agent_core::IAgent;
use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_framework::AgentBuilder;

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
            warn!("No agents were created. Ensure DEEPSEEK_API_KEY (or OPENAI_API_KEY) is set and the provider config is correct.");
        }

        Ok(agents)
    }

    /// Create a chat client from the provider config.
    fn create_client(&self, model_override: Option<&str>) -> Result<DeepSeekChatClient> {
        let provider = &self.config.provider;
        let api_key = provider
            .resolve_api_key()
            .ok_or_else(|| anyhow!(
                "No API key configured. Set DEEPSEEK_API_KEY or OPENAI_API_KEY environment variable, \
                 or configure via CLI: --api-key YOUR_KEY"
            ))?;

        let model = model_override.unwrap_or(&provider.model);

        let options = match provider.provider.as_str() {
            "openai" => {
                let mut opts = ChatClientOptions::openai(model, api_key);
                if let Some(url) = &provider.base_url {
                    opts.api_base = url.clone();
                }
                opts
            }
            "deepseek" | _ => {
                let mut opts = ChatClientOptions::deepseek(model, api_key);
                if let Some(url) = &provider.base_url {
                    opts.api_base = url.clone();
                }
                opts
            }
        };

        Ok(DeepSeekChatClient::new(options)?)
    }

    /// Create the coding agent.
    fn create_coding_agent(&self) -> Result<Arc<dyn IAgent>> {
        let client = self.create_client(None)?;

        let agent = AgentBuilder::new("coding")
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
            .max_tool_rounds(15)
            .build()?;

        Ok(agent)
    }

    /// Create the general agent.
    fn create_general_agent(&self) -> Result<Arc<dyn IAgent>> {
        let client = self.create_client(None)?;

        let agent = AgentBuilder::new("general")
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

        let agent = AgentBuilder::new("analysis")
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
