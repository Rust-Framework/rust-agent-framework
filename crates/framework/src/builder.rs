use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{IAgent, IChatClient, ITool, Result, ToolRegistry};

use crate::ChatClientAgent;
use crate::agents::tool_loop_agent::ToolLoopAgent;

/// Fluently construct an agent with reasonable defaults.
///
/// The `build()` method assembles the following stack:
///   1. `ChatClientAgent` — terminal node that calls the LLM
///   2. `ToolLoopAgent` (if tools are present) — intercepts tool calls
///      and executes them in a loop
///
/// Future layers (HistoryAgent, TracingAgent) will be inserted in later
/// releases as optional decorators.
pub struct AgentBuilder<C> {
    agent_id: String,
    chat_client: Option<C>,
    instructions: String,
    tools: Vec<Arc<dyn ITool>>,
    properties: HashMap<String, serde_json::Value>,
    description: String,
    max_tool_rounds: usize,
}

impl<C: IChatClient + 'static> AgentBuilder<C> {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            agent_id: id.into(),
            chat_client: None,
            instructions: String::new(),
            tools: Vec::new(),
            properties: HashMap::new(),
            description: String::new(),
            max_tool_rounds: 10,
        }
    }

    pub fn chat_client(mut self, client: C) -> Self {
        self.chat_client = Some(client);
        self
    }

    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = text.into();
        self
    }

    pub fn with_tool(mut self, tool: impl ITool + 'static) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    pub fn with_properties(
        mut self,
        iter: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) -> Self {
        for (k, v) in iter {
            self.properties.insert(k, v);
        }
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn max_tool_rounds(mut self, rounds: usize) -> Self {
        self.max_tool_rounds = rounds;
        self
    }

    /// Build the agent stack: ToolLoopAgent wraps ChatClientAgent.
    pub fn build(self) -> Result<Arc<dyn IAgent>> {
        let chat_client = self.chat_client.ok_or_else(|| {
            rust_agent_core::AgentError::ConfigError("chat_client is required".into())
        })?;

        // 1. ChatClientAgent — terminal node
        let mut agent = ChatClientAgent::new(&self.agent_id, Arc::new(chat_client))
            .with_instructions(&self.instructions);

        if !self.description.is_empty() {
            agent = agent.with_description(&self.description);
        }

        if !self.tools.is_empty() {
            let mut registry = ToolRegistry::new();
            for t in &self.tools {
                registry.register_arc(Arc::clone(t));
            }
            agent = agent.with_tools(registry);
        }

        let agent: Arc<dyn IAgent> = Arc::new(agent);

        // 2. ToolLoopAgent — wrap if tools are present
        let agent: Arc<dyn IAgent> = if !self.tools.is_empty() {
            Arc::new(
                ToolLoopAgent::new(
                    format!("{}-tool-loop", self.agent_id),
                    agent,
                    self.tools,
                )
                .with_max_rounds(self.max_tool_rounds),
            )
        } else {
            agent
        };

        // 3. HistoryAgent — TODO in future (auto session management)
        // 4. TracingAgent — TODO in future

        Ok(agent)
    }
}
