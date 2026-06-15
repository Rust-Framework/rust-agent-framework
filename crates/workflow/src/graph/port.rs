use crate::executor::TypeTag;

/// 外部请求端口 — 对应 MAF 的 RequestPort
///
/// 允许工作流在执行过程中向外部发出请求并等待响应，
/// 用于人工审批、工具调用等场景。
#[derive(Debug, Clone)]
pub struct RequestPort {
    pub id: String,
    pub request_type: TypeTag,
    pub response_type: TypeTag,
    pub target_node_id: String,
}

impl RequestPort {
    pub fn new(
        id: impl Into<String>,
        request_type: TypeTag,
        response_type: TypeTag,
        target_node_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            request_type,
            response_type,
            target_node_id: target_node_id.into(),
        }
    }
}
