use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rust_agent_core::{IAgent, Result};

use crate::engine::retry::RetryConfig;
use crate::executor::{AgentExecutor, IExecutor};
use crate::graph::edge::{DirectEdgeData, Edge, EdgeId, FanInEdgeData, FanOutEdgeData, IEdgeCondition, IFanOutAssigner};
use crate::graph::node::Node;
use crate::graph::port::RequestPort;
use crate::graph::WorkflowGraph;
use std::time::Duration;

/// 工作流图构建器 — 对应 MAF 的 WorkflowBuilder
///
/// 提供流式 API 用于声明式构建工作流图：
/// ```ignore
/// WorkflowBuilder::new()
///     .add_agent_node("researcher", researcher)
///     .add_agent_node("writer", writer)
///     .set_start("researcher")
///     .add_edge("researcher", "writer")
///     .with_output_from("writer")
///     .build()?;
/// ```
#[derive(Default)]
pub struct WorkflowBuilder {
    nodes: HashMap<String, Node>,
    edges: Vec<Edge>,
    ports: Vec<RequestPort>,
    output_node_ids: HashSet<String>,
    start_node_id: Option<String>,
    edge_count: u64,
}

impl WorkflowBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个 IExecutor 节点
    pub fn add_node(mut self, id: impl Into<String>, executor: Arc<dyn IExecutor>) -> Self {
        let id = id.into();
        let is_output = executor.is_output();
        let node = Node::new(id.clone(), executor).with_output(is_output);
        self.nodes.insert(id, node);
        self
    }

    /// 快捷方式：将 IAgent 包装为 AgentExecutor 并注册为节点
    pub fn add_agent_node(mut self, id: impl Into<String>, agent: Arc<dyn IAgent>) -> Self {
        let id = id.into();
        let executor = Arc::new(AgentExecutor::new(id.clone(), agent));
        let node = Node::new(id.clone(), executor);
        self.nodes.insert(id, node);
        self
    }

    /// 为最后一次 add_node 添加的节点设置重试策略
    pub fn with_retry(mut self, config: RetryConfig) -> Self {
        if let Some((_, node)) = self.nodes.iter_mut().last() {
            node.retry = Some(config);
        }
        self
    }

    /// 为最后一次 add_node 添加的节点设置超时
    pub fn with_node_timeout(mut self, timeout: Duration) -> Self {
        if let Some((_, node)) = self.nodes.iter_mut().last() {
            node.timeout = Some(timeout);
        }
        self
    }

    /// 指定入口节点
    pub fn set_start(mut self, id: impl Into<String>) -> Self {
        self.start_node_id = Some(id.into());
        self
    }

    /// 标记输出节点
    pub fn with_output_from(mut self, id: impl Into<String>) -> Self {
        self.output_node_ids.insert(id.into());
        self
    }

    // ── 边操作 ──

    /// 添加直接边：source → target
    pub fn add_edge(mut self, source: impl Into<String>, target: impl Into<String>) -> Self {
        let edge_id = self.next_edge_id();
        let edge = Edge::Direct(DirectEdgeData {
            edge_id,
            source_id: source.into(),
            sink_id: target.into(),
            label: None,
            condition: None,
        });
        self.edges.push(edge);
        self
    }

    /// 添加扇出边：source → [targets...]
    pub fn add_fan_out_edge(
        mut self,
        source: impl Into<String>,
        targets: Vec<impl Into<String>>,
    ) -> Self {
        let edge_id = self.next_edge_id();
        let edge = Edge::FanOut(FanOutEdgeData {
            edge_id,
            source_id: source.into(),
            sink_ids: targets.into_iter().map(|t| t.into()).collect(),
            label: None,
            assigner: None,
        });
        self.edges.push(edge);
        self
    }

    /// 添加扇入边：[sources...] → target
    pub fn add_fan_in_edge(
        mut self,
        sources: Vec<impl Into<String>>,
        target: impl Into<String>,
    ) -> Self {
        let edge_id = self.next_edge_id();
        let edge = Edge::FanIn(FanInEdgeData {
            edge_id,
            source_ids: sources.into_iter().map(|s| s.into()).collect(),
            sink_id: target.into(),
            label: None,
        });
        self.edges.push(edge);
        self
    }

    /// 添加带条件的直接边：只有 condition.evaluate() 返回 true 时消息才沿此边传递
    pub fn add_edge_with_condition(
        mut self,
        source: impl Into<String>,
        target: impl Into<String>,
        condition: Arc<dyn IEdgeCondition>,
    ) -> Self {
        let edge_id = self.next_edge_id();
        let edge = Edge::Direct(DirectEdgeData {
            edge_id,
            source_id: source.into(),
            sink_id: target.into(),
            label: None,
            condition: Some(condition),
        });
        self.edges.push(edge);
        self
    }

    /// 添加带分配器的扇出边：由 assigner.targets() 动态决定消息投递目标
    pub fn add_fan_out_edge_with_assigner(
        mut self,
        source: impl Into<String>,
        targets: Vec<impl Into<String>>,
        assigner: Arc<dyn IFanOutAssigner>,
    ) -> Self {
        let edge_id = self.next_edge_id();
        let edge = Edge::FanOut(FanOutEdgeData {
            edge_id,
            source_id: source.into(),
            sink_ids: targets.into_iter().map(|t| t.into()).collect(),
            label: None,
            assigner: Some(assigner),
        });
        self.edges.push(edge);
        self
    }

    /// 添加外部请求端口
    pub fn add_port(mut self, port: RequestPort) -> Self {
        self.ports.push(port);
        self
    }

    // ── 构建 ──

    /// 验证并构建不可变的 WorkflowGraph
    pub fn build(self) -> Result<WorkflowGraph> {
        // 1. 验证入口节点
        let start_node_id = self.start_node_id.ok_or_else(|| {
            rust_agent_core::AgentError::WorkflowError(
                "必须设置入口节点 (set_start)".to_string(),
            )
        })?;

        // 2. 验证入口节点已注册
        if !self.nodes.contains_key(&start_node_id) {
            return Err(rust_agent_core::AgentError::WorkflowError(format!(
                "入口节点 '{}' 未注册",
                start_node_id
            )));
        }

        // 3. 验证所有边引用的节点存在
        for edge in &self.edges {
            for source_id in edge.source_ids() {
                if !self.nodes.contains_key(source_id) {
                    return Err(rust_agent_core::AgentError::WorkflowError(format!(
                        "边源节点 '{}' 未注册",
                        source_id
                    )));
                }
            }
            for sink_id in edge.sink_ids() {
                if !self.nodes.contains_key(sink_id) {
                    return Err(rust_agent_core::AgentError::WorkflowError(format!(
                        "边目标节点 '{}' 未注册",
                        sink_id
                    )));
                }
            }
        }

        // 4. 验证输出节点存在
        for output_id in &self.output_node_ids {
            if !self.nodes.contains_key(output_id) {
                return Err(rust_agent_core::AgentError::WorkflowError(format!(
                    "输出节点 '{}' 未注册",
                    output_id
                )));
            }
        }

        // 5. 构建 edges_by_source 索引
        let mut edges_by_source: HashMap<String, HashSet<Edge>> = HashMap::new();
        for edge in self.edges {
            for source_id in edge.source_ids() {
                edges_by_source
                    .entry(source_id.to_string())
                    .or_default()
                    .insert(edge.clone());
            }
        }

        let graph = WorkflowGraph::new(
            self.nodes,
            edges_by_source,
            self.ports.into_iter().map(|p| (p.id.clone(), p)).collect(),
            self.output_node_ids,
            start_node_id,
        );

        // 6. 运行拓扑校验
        graph.validate()?;

        Ok(graph)
    }

    fn next_edge_id(&mut self) -> EdgeId {
        let id = EdgeId::new(format!("edge_{}", self.edge_count));
        self.edge_count += 1;
        id
    }
}
