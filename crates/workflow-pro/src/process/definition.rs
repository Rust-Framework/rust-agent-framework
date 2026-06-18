use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::Result;
use rust_agent_workflow::engine::retry::RetryOptions;
use rust_agent_workflow::engine::IWorkflowContext;
use rust_agent_workflow::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use rust_agent_workflow::graph::condition::ComparisonOp;
use rust_agent_workflow::graph::edge::IEdgeCondition;
use rust_agent_workflow::graph::WorkflowGraph;
use rust_agent_workflow::WorkflowBuilder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind { Start, End, ServiceTask, UserTask, ScriptTask, SendTask, ReceiveTask, BusinessRuleTask, CallActivity, NoneTask, ParallelGateway, ExclusiveGateway, InclusiveGateway, EventBasedGateway, TimerBoundary, ErrorBoundary }

impl NodeKind {
    pub fn is_gateway(&self) -> bool { matches!(self, NodeKind::ParallelGateway | NodeKind::ExclusiveGateway | NodeKind::InclusiveGateway | NodeKind::EventBasedGateway) }
    pub fn is_boundary_event(&self) -> bool { matches!(self, NodeKind::TimerBoundary | NodeKind::ErrorBoundary) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryDef { pub max_retries: u32, #[serde(default)] pub backoff_ms: u64, #[serde(default)] pub max_backoff_ms: u64 }

impl From<&RetryDef> for RetryOptions {
    fn from(def: &RetryDef) -> Self {
        use rust_agent_workflow::engine::retry::{ExhaustedAction, RetryBackoff, RetryCondition};
        RetryOptions { max_retries: def.max_retries, backoff: if def.backoff_ms == 0 { RetryBackoff::None } else { RetryBackoff::Exponential { base: Duration::from_millis(def.backoff_ms), max: Duration::from_millis(def.max_backoff_ms.max(def.backoff_ms * 10)) } }, retry_on: RetryCondition::AllErrors, on_exhausted: ExhaustedAction::Fail }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeConditionDef { pub condition_type: String, pub expression: Option<String>, pub variable: Option<String>, pub operator: Option<String>, pub expected: Option<serde_json::Value> }

impl EdgeConditionDef {
    pub fn comparison_op(&self) -> Option<ComparisonOp> {
        self.operator.as_deref().and_then(|op| match op { "eq"|"==" => Some(ComparisonOp::Eq), "neq"|"!=" => Some(ComparisonOp::Neq), "gt"|">" => Some(ComparisonOp::Gt), "gte"|">=" => Some(ComparisonOp::Gte), "lt"|"<" => Some(ComparisonOp::Lt), "lte"|"<=" => Some(ComparisonOp::Lte), "contains" => Some(ComparisonOp::Contains), "starts_with" => Some(ComparisonOp::StartsWith), _ => None })
    }
}

pub struct DefinedEdgeCondition { pub variable: String, pub operator: ComparisonOp, pub expected: serde_json::Value, pub state_reader: Option<Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>> }

impl DefinedEdgeCondition {
    pub fn from_def(def: &EdgeConditionDef) -> Option<Self> {
        let variable = def.variable.clone()?; let operator = def.comparison_op()?; let expected = def.expected.clone().unwrap_or(serde_json::Value::Null);
        Some(Self { variable, operator, expected, state_reader: None })
    }
    fn evaluate_value(&self, actual: &serde_json::Value) -> bool {
        match self.operator {
            ComparisonOp::Eq => actual == &self.expected, ComparisonOp::Neq => actual != &self.expected,
            ComparisonOp::Contains => { let a = actual.as_str().unwrap_or_default(); let e = self.expected.as_str().unwrap_or_default(); a.contains(e) }
            ComparisonOp::StartsWith => { let a = actual.as_str().unwrap_or_default(); let e = self.expected.as_str().unwrap_or_default(); a.starts_with(e) }
            ComparisonOp::Gt | ComparisonOp::Gte | ComparisonOp::Lt | ComparisonOp::Lte => {
                let a = actual.as_f64(); let e = self.expected.as_f64();
                match (a, e) { (Some(a), Some(e)) => match self.operator { ComparisonOp::Gt => a > e, ComparisonOp::Gte => a >= e, ComparisonOp::Lt => a < e, ComparisonOp::Lte => a <= e, _ => false }, _ => false }
            }
        }
    }
}

#[async_trait]
impl IEdgeCondition for DefinedEdgeCondition {
    async fn evaluate(&self, _envelope: &rust_agent_workflow::engine::message_envelope::MessageEnvelope) -> Result<bool> {
        if let Some(ref reader) = self.state_reader { let state = reader.lock().await; let value = state.get(&self.variable).cloned().unwrap_or(serde_json::Value::Null); Ok(self.evaluate_value(&value)) } else { Ok(false) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDef { pub id: String, pub kind: NodeKind, #[serde(default)] pub label: Option<String>, #[serde(default)] pub description: Option<String>, #[serde(default)] pub config: serde_json::Value, #[serde(default)] pub retry: Option<RetryDef>, #[serde(default)] pub timeout_ms: Option<u64> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDef { pub id: String, pub source: String, pub target: String, #[serde(default)] pub label: Option<String>, #[serde(default)] pub condition: Option<EdgeConditionDef> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDef { pub name: String, #[serde(default)] pub default_value: Option<serde_json::Value>, #[serde(default)] pub required: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerDef { pub node_id: String, pub kind: String, pub value: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryEventDef { pub attached_to: String, pub event_node_id: String, pub kind: String, #[serde(default)] pub interrupting: bool, #[serde(default)] pub timer_duration_ms: Option<u64>, #[serde(default)] pub error_code: Option<String>, #[serde(default)] pub signal_name: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDefinition { pub id: String, #[serde(default)] pub name: String, #[serde(default)] pub version: String, #[serde(default)] pub description: Option<String>, #[serde(default)] pub nodes: Vec<NodeDef>, #[serde(default)] pub edges: Vec<EdgeDef>, #[serde(default)] pub variables: Vec<VariableDef>, #[serde(default)] pub timers: Vec<TimerDef>, #[serde(default)] pub events: Vec<BoundaryEventDef> }

impl ProcessDefinition {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), name: String::new(), version: String::new(), description: None, nodes: Vec::new(), edges: Vec::new(), variables: Vec::new(), timers: Vec::new(), events: Vec::new() }
    }
    pub fn compile(&self) -> Result<WorkflowGraph> {
        if self.nodes.is_empty() { return Err(rust_agent_core::AgentError::WorkflowError("Process definition has no nodes".into())); }
        let start_node = self.nodes.iter().find(|n| n.kind == NodeKind::Start).ok_or_else(|| rust_agent_core::AgentError::WorkflowError("Process definition has no Start node".into()))?;
        let end_node_ids: Vec<&str> = self.nodes.iter().filter(|n| n.kind == NodeKind::End).map(|n| n.id.as_str()).collect();
        if end_node_ids.is_empty() { return Err(rust_agent_core::AgentError::WorkflowError("Process definition has no End node".into())); }
        let boundary_node_ids: std::collections::HashSet<&str> = self.events.iter().map(|ev| ev.event_node_id.as_str()).collect();
        let mut builder = WorkflowBuilder::new();
        for node in &self.nodes {
            if boundary_node_ids.contains(node.id.as_str()) { continue; }
            let executor = ProcessTaskExecutor { node_id: node.id.clone(), kind: node.kind.clone() };
            builder = builder.add_node(node.id.clone(), Arc::new(executor));
            if let Some(ref retry_def) = node.retry { builder = builder.with_retry(RetryOptions::from(retry_def)); }
            if let Some(timeout_ms) = node.timeout_ms { builder = builder.with_node_timeout(Duration::from_millis(timeout_ms)); }
        }
        for ev in &self.events {
            let boundary_kind = match ev.kind.as_str() { "timer" => NodeKind::TimerBoundary, "error" => NodeKind::ErrorBoundary, other => return Err(rust_agent_core::AgentError::WorkflowError(format!("Unknown boundary event kind '{}'", other))) };
            let executor = ProcessTaskExecutor { node_id: ev.event_node_id.clone(), kind: boundary_kind };
            builder = builder.add_node(ev.event_node_id.clone(), Arc::new(executor));
        }
        let mut edges_by_source: HashMap<&str, Vec<&EdgeDef>> = HashMap::new();
        for edge in &self.edges { edges_by_source.entry(edge.source.as_str()).or_default().push(edge); }
        for (source_id, outgoing_edges) in &edges_by_source {
            let source_node = self.nodes.iter().find(|n| n.id == *source_id);
            match source_node.map(|n| &n.kind) {
                Some(NodeKind::ExclusiveGateway) => {
                    let mut branches: Vec<(String, Arc<dyn IEdgeCondition>)> = Vec::new();
                    let mut default_branch: Option<String> = None;
                    for edge in outgoing_edges.iter() {
                        if let Some(ref cond_def) = edge.condition {
                            if let Some(condition) = DefinedEdgeCondition::from_def(cond_def) { branches.push((edge.target.clone(), Arc::new(condition))); }
                            else { default_branch = Some(edge.target.clone()); }
                        } else { default_branch = Some(edge.target.clone()); }
                    }
                    builder = builder.exclusive_gateway(*source_id, branches, default_branch);
                }
                Some(NodeKind::ParallelGateway) => {
                    let targets: Vec<String> = outgoing_edges.iter().map(|e| e.target.clone()).collect();
                    builder = builder.parallel_gateway(*source_id, targets);
                }
                Some(NodeKind::InclusiveGateway) => {
                    let branches: Vec<(String, Arc<dyn IEdgeCondition>)> = outgoing_edges.iter().filter_map(|edge| edge.condition.as_ref().and_then(|cond_def| DefinedEdgeCondition::from_def(cond_def).map(|c| (edge.target.clone(), Arc::new(c) as Arc<dyn IEdgeCondition>)))).collect();
                    builder = builder.inclusive_gateway(*source_id, branches);
                }
                _ => {
                    for edge in outgoing_edges.iter() {
                        if let Some(ref cond_def) = edge.condition {
                            if let Some(condition) = DefinedEdgeCondition::from_def(cond_def) { builder = builder.add_edge_with_condition(edge.source.clone(), edge.target.clone(), Arc::new(condition)); }
                            else { builder = builder.add_edge(edge.source.clone(), edge.target.clone()); }
                        } else { builder = builder.add_edge(edge.source.clone(), edge.target.clone()); }
                    }
                }
            }
        }
        for ev in &self.events {
            let already_connected = self.edges.iter().any(|e| e.source == ev.attached_to && e.target == ev.event_node_id);
            if !already_connected { builder = builder.add_edge(ev.attached_to.clone(), ev.event_node_id.clone()); }
        }
        builder = builder.set_start(start_node.id.clone());
        for end_id in &end_node_ids { builder = builder.with_output_from(*end_id); }
        builder.build()
    }
    pub fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() { return Err(rust_agent_core::AgentError::WorkflowError("Process definition must have at least one node".into())); }
        if !self.nodes.iter().any(|n| n.kind == NodeKind::Start) { return Err(rust_agent_core::AgentError::WorkflowError("Process definition must have a Start node".into())); }
        if !self.nodes.iter().any(|n| n.kind == NodeKind::End) { return Err(rust_agent_core::AgentError::WorkflowError("Process definition must have at least one End node".into())); }
        let node_ids: std::collections::HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        let event_node_ids: std::collections::HashSet<&str> = self.events.iter().map(|e| e.event_node_id.as_str()).collect();
        let all_ids: std::collections::HashSet<&str> = node_ids.union(&event_node_ids).copied().collect();
        for edge in &self.edges {
            if !all_ids.contains(edge.source.as_str()) { return Err(rust_agent_core::AgentError::WorkflowError(format!("Edge '{}' references unknown source node '{}'", edge.id, edge.source))); }
            if !all_ids.contains(edge.target.as_str()) { return Err(rust_agent_core::AgentError::WorkflowError(format!("Edge '{}' references unknown target node '{}'", edge.id, edge.target))); }
        }
        for ev in &self.events {
            if !node_ids.contains(ev.attached_to.as_str()) { return Err(rust_agent_core::AgentError::WorkflowError(format!("Boundary event '{}' references unknown node '{}'", ev.event_node_id, ev.attached_to))); }
        }
        Ok(())
    }
}

struct ProcessTaskExecutor { node_id: String, kind: NodeKind }

/// Placeholder IExecutor that passes messages through for compiled ProcessDefinitions.
/// TODO: Replace with actual activity bindings (ServiceTask→ServiceTask, ScriptTask→ScriptTask, etc.)
#[async_trait]
impl IExecutor for ProcessTaskExecutor {
    fn id(&self) -> &str { &self.node_id }
    fn is_output(&self) -> bool { self.kind == NodeKind::End }
    fn accepted_types(&self) -> Vec<TypeTag> { vec![TypeTag::new("initial"), TypeTag::new(std::any::type_name::<String>())] }
    async fn handle(&self, message: Arc<dyn std::any::Any + Send + Sync>, _ctx: Arc<dyn IWorkflowContext>, _progress: tokio::sync::mpsc::UnboundedSender<NodeProgress>) -> Result<HandlerResult> { Ok(HandlerResult::Messages(vec![message])) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_simple_process() -> ProcessDefinition {
        ProcessDefinition {
            id: "test_process".into(), name: "Test Process".into(), version: "1.0".into(), description: None,
            nodes: vec![
                NodeDef { id: "start".into(), kind: NodeKind::Start, label: Some("Start".into()), description: None, config: serde_json::Value::Null, retry: None, timeout_ms: None },
                NodeDef { id: "task1".into(), kind: NodeKind::ServiceTask, label: Some("Task 1".into()), description: None, config: serde_json::Value::Null, retry: None, timeout_ms: Some(5000) },
                NodeDef { id: "end".into(), kind: NodeKind::End, label: Some("End".into()), description: None, config: serde_json::Value::Null, retry: None, timeout_ms: None },
            ],
            edges: vec![
                EdgeDef { id: "e1".into(), source: "start".into(), target: "task1".into(), label: None, condition: None },
                EdgeDef { id: "e2".into(), source: "task1".into(), target: "end".into(), label: None, condition: None },
            ],
            variables: vec![VariableDef { name: "input".into(), default_value: Some(serde_json::Value::String("default".into())), required: false }],
            timers: vec![], events: vec![],
        }
    }

    #[test] fn test_validate_simple_process() { assert!(make_simple_process().validate().is_ok()); }
    #[test] fn test_validate_missing_start() { let mut p = make_simple_process(); p.nodes[0].kind = NodeKind::ServiceTask; assert!(p.validate().is_err()); }

    #[test] fn test_compile_simple_process() {
        let graph = make_simple_process().compile().expect("ok");
        assert_eq!(graph.start_node_id(), "start");
        assert!(graph.nodes().contains_key("start"));
        assert!(graph.nodes().contains_key("task1"));
        assert!(graph.nodes().contains_key("end"));
        assert!(graph.output_node_ids().contains("end"));
    }

    #[test] fn test_compile_with_gateway() {
        let process = ProcessDefinition {
            id: "gwp".into(), name: String::new(), version: String::new(), description: None,
            nodes: vec![
                NodeDef { id: "start".into(), kind: NodeKind::Start, label: None, description: None, config: serde_json::Value::Null, retry: None, timeout_ms: None },
                NodeDef { id: "gate".into(), kind: NodeKind::ExclusiveGateway, label: None, description: None, config: serde_json::Value::Null, retry: None, timeout_ms: None },
                NodeDef { id: "a".into(), kind: NodeKind::ServiceTask, label: None, description: None, config: serde_json::Value::Null, retry: None, timeout_ms: None },
                NodeDef { id: "b".into(), kind: NodeKind::ServiceTask, label: None, description: None, config: serde_json::Value::Null, retry: None, timeout_ms: None },
                NodeDef { id: "end".into(), kind: NodeKind::End, label: None, description: None, config: serde_json::Value::Null, retry: None, timeout_ms: None },
            ],
            edges: vec![
                EdgeDef { id: "e1".into(), source: "start".into(), target: "gate".into(), label: None, condition: None },
                EdgeDef { id: "e2".into(), source: "gate".into(), target: "a".into(), label: None, condition: Some(EdgeConditionDef { condition_type: "variable".into(), expression: None, variable: Some("score".into()), operator: Some("gte".into()), expected: Some(serde_json::json!(80)) }) },
                EdgeDef { id: "e3".into(), source: "gate".into(), target: "b".into(), label: None, condition: None },
                EdgeDef { id: "e4".into(), source: "a".into(), target: "end".into(), label: None, condition: None },
                EdgeDef { id: "e5".into(), source: "b".into(), target: "end".into(), label: None, condition: None },
            ],
            variables: vec![], timers: vec![], events: vec![],
        };
        let g = process.compile().expect("ok"); assert!(g.nodes().contains_key("gate"));
    }

    #[test] fn test_node_kind_serialization() {
        let json = serde_json::json!(["start","end","service_task","user_task","script_task","send_task","receive_task","business_rule_task","call_activity","none_task","parallel_gateway","exclusive_gateway","inclusive_gateway","event_based_gateway","timer_boundary","error_boundary"]);
        let kinds: Vec<NodeKind> = serde_json::from_value(json).unwrap();
        assert_eq!(kinds.len(), 16); assert_eq!(kinds[0], NodeKind::Start); assert_eq!(kinds[10], NodeKind::ParallelGateway);
    }

    #[test] fn test_serialization() {
        let p = make_simple_process();
        let json = serde_json::to_string_pretty(&p).unwrap();
        let d: ProcessDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(d.id, "test_process"); assert_eq!(d.nodes.len(), 3); assert_eq!(d.edges.len(), 2);
    }
}
