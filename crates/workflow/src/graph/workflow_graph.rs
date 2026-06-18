use std::collections::{HashMap, HashSet, VecDeque};

use rust_agent_core::Result;

use super::edge::Edge;
use super::node::Node;
use super::port::RequestPort;

/// 不可变的工作流图定义 — 对应 MAF 的 Workflow
///
/// 通过 WorkflowBuilder 构建，`build()` 后冻结。
/// 通过 `Arc` 共享节点实例（Rust 天然支持，无需 MAF 的 CAS 所有权模型）。
#[derive(Clone)]
pub struct WorkflowGraph {
    /// 全部节点，按 ID 索引
    pub(crate) nodes: HashMap<String, Node>,
    /// 边，按源节点 ID 分组
    pub(crate) edges: HashMap<String, HashSet<Edge>>,
    /// 外部请求端口
    pub(crate) ports: HashMap<String, RequestPort>,
    /// 标记为输出的节点 ID 集合
    pub(crate) output_node_ids: HashSet<String>,
    /// 入口节点 ID
    pub(crate) start_node_id: String,
}

impl WorkflowGraph {
    /// 创建空图（仅供 Builder 使用）
    pub(crate) fn new(
        nodes: HashMap<String, Node>,
        edges: HashMap<String, HashSet<Edge>>,
        ports: HashMap<String, RequestPort>,
        output_node_ids: HashSet<String>,
        start_node_id: String,
    ) -> Self {
        Self {
            nodes,
            edges,
            ports,
            output_node_ids,
            start_node_id,
        }
    }

    /// 从入口出发的 BFS 可达性校验 + 环检测
    pub fn validate(&self) -> Result<()> {
        // 1. 入口节点存在
        if !self.nodes.contains_key(&self.start_node_id) {
            return Err(rust_agent_core::AgentError::WorkflowError(format!(
                "入口节点 '{}' 未注册",
                self.start_node_id
            )));
        }

        // 2. 所有边引用的节点必须存在
        for (source_id, edge_set) in &self.edges {
            if !self.nodes.contains_key(source_id) {
                return Err(rust_agent_core::AgentError::WorkflowError(format!(
                    "边源节点 '{}' 未注册",
                    source_id
                )));
            }
            for edge in edge_set {
                for sink_id in edge.sink_ids() {
                    if !self.nodes.contains_key(sink_id) {
                        return Err(rust_agent_core::AgentError::WorkflowError(format!(
                            "边目标节点 '{}' 未注册",
                            sink_id
                        )));
                    }
                }
            }
        }

        // 3. BFS 可达性检查（从入口出发）
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(self.start_node_id.as_str());
        visited.insert(self.start_node_id.as_str());

        while let Some(current) = queue.pop_front() {
            if let Some(edge_set) = self.edges.get(current) {
                for edge in edge_set {
                    for sink_id in edge.sink_ids() {
                        if !visited.contains(sink_id) {
                            visited.insert(sink_id);
                            queue.push_back(sink_id);
                        }
                    }
                }
            }
        }

        let reachable: HashSet<&str> = visited;
        let all_nodes: HashSet<&str> = self.nodes.keys().map(|s| s.as_str()).collect();

        // 警告未可达节点（非严格错误，可以存在死代码节点）
        for unreachable in all_nodes.difference(&reachable) {
            tracing::warn!("节点 '{}' 从入口不可达", unreachable);
        }

        // 4. 环检测（DFS 三色标记法，仅检测可达子图）
        self.detect_cycles(&reachable)?;

        Ok(())
    }

    /// 使用 DFS 三色标记法检测有向图中的环。
    /// 显式标记 `is_loopback = true` 的边不会被检测为环。
    fn detect_cycles(&self, reachable: &HashSet<&str>) -> Result<()> {
        #[derive(Clone, Copy, PartialEq)]
        enum Color {
            White,
            Gray,
            Black,
        }

        let mut colors: HashMap<&str, Color> = reachable
            .iter()
            .map(|id| (*id, Color::White))
            .collect();

        fn is_loopback_edge(edge: &Edge) -> bool {
            match edge {
                Edge::Direct(d) => d.is_loopback,
                _ => false,
            }
        }

        fn dfs<'a>(
            node: &'a str,
            colors: &mut HashMap<&'a str, Color>,
            edges: &'a HashMap<String, HashSet<Edge>>,
        ) -> Result<()> {
            colors.insert(node, Color::Gray);
            if let Some(edge_set) = edges.get(node) {
                for edge in edge_set {
                    // 跳过显式标记的循环回边
                    if is_loopback_edge(edge) {
                        continue;
                    }
                    for sink in edge.sink_ids() {
                        if let Some(&color) = colors.get(sink) {
                            match color {
                                Color::Gray => {
                                    return Err(rust_agent_core::AgentError::WorkflowError(
                                        format!("检测到循环: '{}' -> '{}'。如需循环，请使用 add_loopback_edge() 或标记 is_loopback=true", node, sink),
                                    ));
                                }
                                Color::White => {
                                    dfs(sink, colors, edges)?;
                                }
                                Color::Black => {}
                            }
                        }
                    }
                }
            }
            colors.insert(node, Color::Black);
            Ok(())
        }

        // 从入口节点开始 DFS
        dfs(self.start_node_id.as_str(), &mut colors, &self.edges)?;
        Ok(())
    }

    // ── 访问器 ──

    pub fn nodes(&self) -> &HashMap<String, Node> {
        &self.nodes
    }

    pub fn edges(&self) -> &HashMap<String, HashSet<Edge>> {
        &self.edges
    }

    pub fn start_node_id(&self) -> &str {
        &self.start_node_id
    }

    pub fn output_node_ids(&self) -> &HashSet<String> {
        &self.output_node_ids
    }

    pub fn ports(&self) -> &HashMap<String, RequestPort> {
        &self.ports
    }

    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn get_edges_from(&self, source_id: &str) -> Option<&HashSet<Edge>> {
        self.edges.get(source_id)
    }
}
