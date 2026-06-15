use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::{
    AgentId, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent,
    ISession, Result, Content,
};

/// Handoff orchestration pattern — triage agent routes to the best-fit target agent.
/// Corresponds to MAF's handoff pattern (OpenAI Swarm style).
///
/// # 执行流程
///
/// 1. Triage agent 接收用户输入 + 可用代理清单作为 system instructions
/// 2. Triage agent 流式输出，同时收集文本
/// 3. 解析 triage 响应中的代理名称
/// 4. 匹配目标代理并执行（流式输出）
///
/// # 使用方式
///
/// ```ignore
/// let pattern = HandoffPattern::new()
///     .triage(triage_agent)
///     .agent(code_agent)
///     .agent(writing_agent)
///     .agent(analysis_agent);
///
/// let stream = pattern.run(input, session, options).await?;
/// ```
pub struct HandoffWorkflow {
    /// All agents: [triage, agent1, agent2, ...]
    agents: Vec<Arc<dyn IAgent>>,
    /// Human-readable names for agents (parallel to `agents[1..]`)
    agent_names: Vec<String>,
}

impl Clone for HandoffWorkflow {
    fn clone(&self) -> Self {
        Self { agents: self.agents.clone(), agent_names: self.agent_names.clone() }
    }
}

impl HandoffWorkflow {
    /// 创建新的 HandoffWorkflow 构建器
    pub fn new() -> HandoffBuilder {
        HandoffBuilder {
            triage: None,
            agents: Vec::new(),
            names: Vec::new(),
        }
    }

    /// Execute the handoff pattern.
    ///
    /// 1. Triage agent runs with enriched instructions listing available agents
    /// 2. Collect triage response text
    /// 3. Find matching target agent by name
    /// 4. Execute target agent with original input + triage context
    pub async fn run(
        &self,
        input: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        if self.agents.is_empty() {
            return Err(rust_agent_core::AgentError::WorkflowError(
                "HandoffPattern requires at least 1 agent (triage)".to_string(),
            ));
        }

        let triage = &self.agents[0];
        let targets = &self.agents[1..];
        let names = &self.agent_names;

        // Build triage instructions with agent manifest
        let agent_list = names
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{}. {}", i + 1, name))
            .collect::<Vec<_>>()
            .join("\n");

        let triage_instruction = format!(
            "You are a routing assistant. Analyze the user's request and choose \
             which specialist should handle it.\n\n\
             Available specialists:\n{}\n\n\
             Reply with ONLY the exact identifier (e.g., \"代码专家\") on a single line. \
             Do not explain your choice.",
            agent_list
        );

        // Build input with system instruction appended
        let mut triage_input = vec![ChatMessage::system(&triage_instruction)];
        triage_input.extend(input.clone());

        // Step 1: Run triage agent and collect full response
        let triage_stream = triage.run(triage_input, session.clone(), options.clone()).await?;
        let results: Vec<_> = triage_stream.collect().await;
        let triage_text = collect_text_from_results(&results);

        tracing::debug!(
            triage_text = %triage_text,
            available_agents = ?names,
            "HandoffPattern: triage completed"
        );

        // Step 2: Find matching target agent
        let target_index = names.iter().position(|name| {
            triage_text.to_lowercase().contains(&name.to_lowercase())
        });

        let target = match target_index {
            Some(idx) => &targets[idx],
            None => {
                // No match found — return the triage response as-is
                tracing::warn!(
                    triage_text = %triage_text,
                    "HandoffPattern: no matching agent found in triage response, returning triage output"
                );
                return Ok(Box::pin(futures_util::stream::iter(results.into_iter())));
            }
        };

        tracing::info!(
            target_agent = %target.id(),
            "HandoffPattern: routing to agent"
        );

        // Step 3: Execute target agent with context
        let mut target_input = vec![ChatMessage::system(&format!(
            "You were selected to handle this request. Triage analysis: {}",
            triage_text
        ))];
        target_input.extend(input);

        target.run(target_input, session, options).await
    }

    /// 查找指定 ID 的代理
    pub fn find_agent(&self, id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.agents.iter().find(|a| a.id() == id)
    }

    /// 将工作流包装为 `IAgent`。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        let all_agents = self.agents.clone();
        let name = format!("handoff_{}", all_agents.iter().map(|a| a.id().to_string()).collect::<Vec<_>>().join("_"));
        Arc::new(super::WorkflowAsAgent::new(name, all_agents, {
            move |input, session, options| {
                let value = self.clone();
                Box::pin(async move { value.run(input, session, options).await })
            }
        }))
    }
}

/// HandoffPattern builder
pub struct HandoffBuilder {
    triage: Option<Arc<dyn IAgent>>,
    agents: Vec<Arc<dyn IAgent>>,
    names: Vec<String>,
}

impl HandoffBuilder {
    /// 设置 triage 代理（接收请求并决定路由）
    pub fn triage(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.triage = Some(agent);
        self
    }

    /// 添加一个目标代理
    pub fn agent(mut self, agent: Arc<dyn IAgent>) -> Self {
        // Use agent's description for matching; fall back to agent ID
        let meta = agent.metadata();
        let name = if meta.description.is_empty() {
            agent.id().to_string()
        } else {
            meta.description.clone()
        };
        self.names.push(name);
        self.agents.push(agent);
        self
    }

    /// 构建 HandoffPattern
    pub fn build(self) -> Result<HandoffWorkflow> {
        let triage = self.triage.ok_or_else(|| {
            rust_agent_core::AgentError::WorkflowError(
                "HandoffPattern requires a triage agent".to_string(),
            )
        })?;

        if self.agents.is_empty() {
            return Err(rust_agent_core::AgentError::WorkflowError(
                "HandoffPattern requires at least one target agent".to_string(),
            ));
        }

        let mut all = vec![triage];
        all.extend(self.agents);
        Ok(HandoffWorkflow {
            agents: all,
            agent_names: self.names,
        })
    }
}

fn collect_text_from_results(results: &[Result<AgentResponseResult>]) -> String {
    let mut text = String::new();
    for result in results {
        if let Ok(r) = result {
            for content in &r.contents {
                if let Content::Text(ref t) = content {
                    text.push_str(&t.delta);
                }
            }
        }
    }
    text
}
