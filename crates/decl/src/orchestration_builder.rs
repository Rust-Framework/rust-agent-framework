//! 将 `OrchestrationDecl` + 子 Agent 实例化为 `Arc<dyn IAgent>`。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent,
    ISession, Result,
};
use rust_agent_workflow::executor::{AgentExecutor, FunctionExecutor};
use rust_agent_workflow::graph::LoopConfig;
use rust_agent_workflow::orchestrations::vote::{
    MajorityAggregator, UnanimousAggregator, WeightedAggregator,
};
use rust_agent_workflow::{
    ConcurrentWorkflowBuilder, GroupChatWorkflowBuilder, HandoffWorkflowBuilder,
    MagenticWorkflowBuilder, SequentialWorkflowBuilder, VoteWorkflowBuilder, WorkflowBuilder,
    WorkflowAgent,
};

use crate::definition::AgentDefinition;
use crate::error::{DeclError, Result as DeclResult};
use crate::orchestration_decl::{OrchestrationDecl, OrchestrationMode, PipelinePhaseDecl};

/// 构建编排 Agent，保留 YAML 声明的 `name` 作为对外 ID。
pub fn build_orchestration_agent(
    def: &AgentDefinition,
    orch: &OrchestrationDecl,
    orchestrator: Option<Arc<dyn IAgent>>,
    sub_agents: HashMap<String, Arc<dyn IAgent>>,
) -> DeclResult<Arc<dyn IAgent>> {
    if sub_agents.is_empty() && orchestrator.is_none() {
        return Err(DeclError::Validation(
            "Orchestration requires at least one agent (orchestrator or subAgents)".into(),
        ));
    }

    let participants: Vec<Arc<dyn IAgent>> = sub_agents.values().cloned().collect();
    let orchestrator_keep = orchestrator.clone();

    let inner: Arc<dyn IAgent> = match orch.mode {
        OrchestrationMode::Magentic => {
            let orchestrator = orchestrator.ok_or_else(|| {
                DeclError::Validation("magentic mode requires a root orchestrator agent".into())
            })?;
            let max = orch.max_iterations.unwrap_or(15);
            let mut b = MagenticWorkflowBuilder::new()
                .orchestrator(orchestrator)
                .max_iterations(max);
            for a in participants {
                b = b.add_sub_agent(a);
            }
            b.build()?.as_agent()
        }

        OrchestrationMode::Sequential => {
            let mut agents: Vec<Arc<dyn IAgent>> = Vec::new();
            if let Some(o) = orchestrator {
                agents.push(o);
            }
            agents.extend(participants);
            SequentialWorkflowBuilder::new()
                .with_agents(agents)
                .build()?
                .as_agent()
        }

        OrchestrationMode::Concurrent => {
            let mut agents: Vec<Arc<dyn IAgent>> = Vec::new();
            if let Some(o) = orchestrator {
                agents.push(o);
            }
            agents.extend(participants);
            ConcurrentWorkflowBuilder::new()
                .with_agents(agents)
                .build()?
                .as_agent()
        }

        OrchestrationMode::Handoff => {
            let triage_name = orch.triage.clone();
            let (triage, experts) = split_handoff_agents(orchestrator, &sub_agents, triage_name)?;
            let mut b = HandoffWorkflowBuilder::new().triage(triage);
            for e in experts {
                b = b.add_agent(e);
            }
            b.build()?.as_agent()
        }

        OrchestrationMode::GroupChat => {
            let coord_name = orch.coordinator.clone();
            let (coordinator, chat_participants) =
                split_group_chat_agents(orchestrator, &sub_agents, coord_name)?;
            let mut b = GroupChatWorkflowBuilder::new();
            if let Some(c) = coordinator {
                b = b.coordinator(c);
            }
            for p in chat_participants {
                b = b.add_participant(p);
            }
            if let Some(rounds) = orch.max_rounds {
                b = b.max_rounds(rounds);
            }
            b.build()?.as_agent()
        }

        OrchestrationMode::Vote => {
            let mut b = VoteWorkflowBuilder::new();
            if let Some(o) = orchestrator {
                b = b.add_voter(o);
            }
            for v in participants {
                b = b.add_voter(v);
            }
            b = apply_vote_aggregator(b, orch);
            if let Some(rounds) = orch.voting_rounds {
                b = b.voting_rounds(rounds);
            }
            b.build()?.as_agent()
        }

        OrchestrationMode::Pipeline => {
            build_pipeline_agent(orch, orchestrator, &sub_agents)?
        }

        OrchestrationMode::Workflow | OrchestrationMode::Custom => {
            return Err(DeclError::Unsupported(
                "workflow/custom orchestration must use kind: workflow at root".into(),
            ));
        }
    };

    let mut all: Vec<Arc<dyn IAgent>> = sub_agents.values().cloned().collect();
    if let Some(o) = orchestrator_keep {
        if !all.iter().any(|a| a.id() == o.id()) {
            all.push(o);
        }
    }

    Ok(wrap_named_agent(def, inner, all))
}

fn split_handoff_agents(
    orchestrator: Option<Arc<dyn IAgent>>,
    sub_agents: &HashMap<String, Arc<dyn IAgent>>,
    triage_name: Option<String>,
) -> DeclResult<(Arc<dyn IAgent>, Vec<Arc<dyn IAgent>>)> {
    if let Some(name) = triage_name {
        let triage = sub_agents.get(&name).cloned().ok_or_else(|| {
            DeclError::Validation(format!(
                "Handoff triage agent '{}' not found in subAgents",
                name
            ))
        })?;
        let experts: Vec<_> = sub_agents
            .iter()
            .filter(|(n, _)| *n != &name)
            .map(|(_, a)| a.clone())
            .collect();
        if experts.is_empty() {
            return Err(DeclError::Validation(
                "Handoff requires at least one expert subAgent besides triage".into(),
            ));
        }
        return Ok((triage, experts));
    }

    let triage = orchestrator.ok_or_else(|| {
        DeclError::Validation(
            "Handoff requires orchestration.triage or a root orchestrator agent".into(),
        )
    })?;
    Ok((triage, sub_agents.values().cloned().collect()))
}

fn split_group_chat_agents(
    orchestrator: Option<Arc<dyn IAgent>>,
    sub_agents: &HashMap<String, Arc<dyn IAgent>>,
    coordinator_name: Option<String>,
) -> DeclResult<(Option<Arc<dyn IAgent>>, Vec<Arc<dyn IAgent>>)> {
    if let Some(name) = coordinator_name {
        let coord = sub_agents.get(&name).cloned().ok_or_else(|| {
            DeclError::Validation(format!(
                "GroupChat coordinator '{}' not found in subAgents",
                name
            ))
        })?;
        let participants: Vec<_> = sub_agents
            .iter()
            .filter(|(n, _)| *n != &name)
            .map(|(_, a)| a.clone())
            .collect();
        return Ok((Some(coord), participants));
    }
    Ok((orchestrator, sub_agents.values().cloned().collect()))
}

fn apply_vote_aggregator(
    b: VoteWorkflowBuilder,
    orch: &OrchestrationDecl,
) -> VoteWorkflowBuilder {
    match orch.aggregator.as_deref().unwrap_or("majority") {
        "unanimous" => b.aggregator(UnanimousAggregator),
        "weighted" if !orch.weights.is_empty() => {
            b.aggregator(WeightedAggregator::new(orch.weights.clone()))
        }
        _ => b.aggregator(MajorityAggregator),
    }
}

fn build_pipeline_agent(
    orch: &OrchestrationDecl,
    orchestrator: Option<Arc<dyn IAgent>>,
    sub_agents: &HashMap<String, Arc<dyn IAgent>>,
) -> DeclResult<Arc<dyn IAgent>> {
    if orch.phases.is_empty() {
        return Err(DeclError::Validation(
            "pipeline mode requires orchestration.phases".into(),
        ));
    }

    let max_iterations = orch.max_iterations.unwrap_or(15);
    let loop_from = orch.loop_from_phase.unwrap_or(0);

    let mut builder = WorkflowBuilder::new();
    let mut prev_exit: Vec<String> = Vec::new();
    let mut loop_entry: Option<String> = None;
    let mut last_exit = String::new();
    let mut need_start = true;

    if let Some(orchestrator) = orchestrator {
        let node_id = "pipeline_orchestrator".to_string();
        builder = builder.add_node(
            node_id.clone(),
            Arc::new(AgentExecutor::new(&node_id, orchestrator)),
        );
        builder = builder.set_start(&node_id);
        prev_exit = vec![node_id.clone()];
        last_exit = node_id;
        need_start = false;
    }

    for (phase_idx, phase) in orch.phases.iter().enumerate() {
        let set_start = need_start && prev_exit.is_empty();
        let (entries, exits, b) = emit_pipeline_phase(
            builder,
            phase_idx,
            phase,
            sub_agents,
            &prev_exit,
            set_start,
        )?;
        builder = b;
        if phase_idx == loop_from {
            loop_entry = entries.first().cloned();
        }
        last_exit = exits.last().cloned().unwrap_or(last_exit);
        prev_exit = exits;
        need_start = false;
    }

    if max_iterations > 0 {
        if let (Some(entry), true) = (loop_entry, !last_exit.is_empty()) {
            builder = builder.with_loop_on(&entry, LoopConfig::new(max_iterations));
            builder = builder.add_loopback_edge(&last_exit, &entry);
        }
    }

    builder = builder.with_output_from(last_exit);
    let graph = builder.build()?;
    Ok(Arc::new(WorkflowAgent::new(graph)))
}

fn emit_pipeline_phase(
    mut builder: WorkflowBuilder,
    phase_idx: usize,
    phase: &PipelinePhaseDecl,
    sub_agents: &HashMap<String, Arc<dyn IAgent>>,
    prev_exit: &[String],
    set_start: bool,
) -> DeclResult<(Vec<String>, Vec<String>, WorkflowBuilder)> {
    match phase {
        PipelinePhaseDecl::Sequential { agents } => {
            if agents.is_empty() {
                return Err(DeclError::Validation(format!(
                    "Pipeline sequential phase {} has no agents",
                    phase_idx
                )));
            }

            let mut entries = Vec::new();
            let mut prev: Option<String> = None;

            for name in agents {
                let agent = sub_agents.get(name).ok_or_else(|| {
                    DeclError::Validation(format!(
                        "Pipeline phase {} references unknown agent '{}'",
                        phase_idx, name
                    ))
                })?;
                let node_id = format!("pipe_{phase_idx}_seq_{name}");
                builder = builder.add_node(
                    node_id.clone(),
                    Arc::new(AgentExecutor::new(&node_id, agent.clone())),
                );

                if prev.is_none() {
                    entries.push(node_id.clone());
                    builder = connect_prev_phase(builder, prev_exit, &node_id);
                    if set_start {
                        builder = builder.set_start(&node_id);
                    }
                }
                if let Some(ref p) = prev {
                    builder = builder.add_edge(p, &node_id);
                }
                prev = Some(node_id);
            }

            let exits = vec![prev.unwrap()];
            Ok((entries, exits, builder))
        }

        PipelinePhaseDecl::Concurrent { agents } => {
            if agents.is_empty() {
                return Err(DeclError::Validation(format!(
                    "Pipeline concurrent phase {} has no agents",
                    phase_idx
                )));
            }

            let source_id = format!("pipe_{phase_idx}_fanout");
            let pass = FunctionExecutor::new(&source_id, |msg: Vec<ChatMessage>| vec![msg]);
            builder = builder.add_node(source_id.clone(), Arc::new(pass));
            builder = connect_prev_phase(builder, prev_exit, &source_id);
            if set_start {
                builder = builder.set_start(&source_id);
            }

            let mut targets = Vec::new();
            for name in agents {
                let agent = sub_agents.get(name).ok_or_else(|| {
                    DeclError::Validation(format!(
                        "Pipeline concurrent phase references unknown agent '{}'",
                        name
                    ))
                })?;
                let node_id = format!("pipe_{phase_idx}_par_{name}");
                builder = builder.add_node(
                    node_id.clone(),
                    Arc::new(AgentExecutor::new(&node_id, agent.clone())),
                );
                targets.push(node_id);
            }

            builder = builder.add_fan_out_edge(&source_id, targets.clone());

            let sink_id = format!("pipe_{phase_idx}_fanin");
            let sink = FunctionExecutor::new(&sink_id, |_msg: String| -> Vec<String> {
                vec!["merged".to_string()]
            });
            builder = builder.add_node(sink_id.clone(), Arc::new(sink));
            builder = builder.add_fan_in_edge(targets, &sink_id);

            Ok((vec![source_id], vec![sink_id], builder))
        }
    }
}

fn connect_prev_phase(
    builder: WorkflowBuilder,
    prev_exit: &[String],
    entry: &str,
) -> WorkflowBuilder {
    let mut b = builder;
    for p in prev_exit {
        if p != entry {
            b = b.add_edge(p, entry);
        }
    }
    b
}

/// 用声明式 `name` 包装工作流 Agent，并注册子 Agent 发现。
pub fn wrap_named_agent(
    def: &AgentDefinition,
    inner: Arc<dyn IAgent>,
    sub_agents: Vec<Arc<dyn IAgent>>,
) -> Arc<dyn IAgent> {
    let mut meta = AgentMetadata::new("OrchestrationAgent", &def.name);
    if !def.description.is_empty() {
        meta.description = def.description.clone();
    } else {
        meta.description = inner.metadata().description.clone();
    }

    Arc::new(NamedOrchestrationAgent {
        id: AgentId::new(&def.name),
        metadata: meta,
        inner,
        sub_agents,
    })
}

struct NamedOrchestrationAgent {
    id: AgentId,
    metadata: AgentMetadata,
    inner: Arc<dyn IAgent>,
    sub_agents: Vec<Arc<dyn IAgent>>,
}

#[async_trait]
impl IAgent for NamedOrchestrationAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.sub_agents
            .iter()
            .find(|a| a.id() == id)
            .cloned()
            .or_else(|| self.inner.get_subagent(id))
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        self.inner.run(messages, session, options).await
    }

    async fn reset(&self) -> Result<()> {
        self.inner.reset().await?;
        for a in &self.sub_agents {
            a.reset().await?;
        }
        Ok(())
    }
}
