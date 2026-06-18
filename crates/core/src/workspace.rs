use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ITool;

/// 工作区跨范围访问策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScopePolicy {
    /// 开发模式——不作任何限制
    AllowAll,
    /// 生产模式——跨范围操作需人机协同审批
    ApproveOutside,
    /// 受限模式——禁止任何跨范围访问
    DenyOutside,
}

/// 工作区范围定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceScope {
    /// 规范化的根路径
    pub root: PathBuf,
    /// 可读名称，注入 system prompt
    pub name: String,
    /// 越界处理策略
    pub policy: ScopePolicy,
    /// 扩展属性——路径白名单、命令白名单、环境变量等
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
}

impl WorkspaceScope {
    /// 创建默认 AllowAll 策略的工作区范围
    pub fn new(root: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            name: name.into(),
            policy: ScopePolicy::AllowAll,
            properties: HashMap::new(),
        }
    }

    /// 设置越界策略
    pub fn with_policy(mut self, policy: ScopePolicy) -> Self {
        self.policy = policy;
        self
    }

    /// 添加扩展属性
    pub fn with_property(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        self.properties.insert(
            key.into(),
            serde_json::to_value(value).unwrap_or_default(),
        );
        self
    }
}

/// 可感知工作区范围的工具接口。
///
/// 实现此 trait 的工具由 `WorkspaceContextProvider` 在 `add_tool()` 时
/// 自动注入 `WorkspaceScope`，无需工具构造函数传参。
pub trait IScopeTool: ITool {
    /// 使用指定工作区范围创建工具的新实例。
    ///
    /// 新实例从 `scope.root` 获取工作目录，从 `scope.policy` 获取越界策略。
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool>;
}
