use std::sync::Arc;

use crate::executor::IExecutor;

/// 图中的一个节点 — 包装 IExecutor 及其元数据
#[derive(Clone)]
pub struct Node {
    pub id: String,
    pub executor: Arc<dyn IExecutor>,
    pub is_output: bool,
}

impl Node {
    pub fn new(id: impl Into<String>, executor: Arc<dyn IExecutor>) -> Self {
        Self {
            id: id.into(),
            executor,
            is_output: false,
        }
    }

    pub fn with_output(mut self, is_output: bool) -> Self {
        self.is_output = is_output;
        self
    }
}
