//! 工具和上下文提供器的分类枚举。
//!
//! 对应 YAML/JSON/TOML 声明式配置中的 `kind` 字段。
//! 使用枚举而非自由格式字符串，提供编译期安全保证。

use serde::{Deserialize, Serialize};

/// 工具分类枚举——对应 YAML 中 `tools[].kind` 的值。
///
/// # YAML/JSON 映射
///
/// 通过 `#[serde(rename_all = "snake_case")]` 实现：
/// - `ToolKind::Function` ↔ `"function"`
/// - `ToolKind::OpenApi` ↔ `"openapi"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// 用户注册的函数工具（通过 `with_tool()` 注册）
    Function,
    /// 工厂注册的自定义工具（通过 `register_factory()` 注册）
    Custom,
    /// Web 搜索/抓取工具（`web_search`、`web_fetch`）
    Web,
    /// 文件系统工具（`read_file`、`write_file` 等 11 个）
    File,
    /// Shell 命令执行工具（`run_command`）
    Shell,
    /// 技能加载和资源工具（`load_skill`、`read_skill_resource`）
    Skills,
    /// 代码解释器/沙箱工具
    Code,
    /// MCP（Model Context Protocol）远程工具
    Mcp,
    /// OpenAPI 规范驱动的工具
    OpenApi,
    /// 未分类工具（向后兼容——旧代码返回 "unknown"）
    Unknown,
}

/// 上下文提供器分类枚举——对应 YAML 中 `contexts[].kind` 的值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextProviderKind {
    /// 持久化跨会话记忆系统
    Memory,
    /// 按需加载的技能文件（SKILL.md）
    Skills,
    /// MCP 远程工具服务器
    Mcp,
    /// 工作区根目录 + 访问策略
    Workspace,
    /// RAG 知识库检索
    Knowledge,
    /// Wiki 知识库
    Wiki,
    /// 对话历史管理（内置于 AgentBuilder）
    History,
    /// 未分类提供器（向后兼容）
    Unknown,
}

impl ToolKind {
    /// 从字符串字面量创建（宏展开阶段使用——不支持的字符串编译失败）。
    pub const fn from_macro_literal(s: &str) -> Self {
        match s.as_bytes() {
            b"function" => ToolKind::Function,
            b"custom" => ToolKind::Custom,
            b"web" => ToolKind::Web,
            b"file" => ToolKind::File,
            b"shell" => ToolKind::Shell,
            b"skills" => ToolKind::Skills,
            b"code" => ToolKind::Code,
            b"mcp" => ToolKind::Mcp,
            b"openapi" => ToolKind::OpenApi,
            _ => ToolKind::Unknown,
        }
    }

    /// 返回 YAML/JSON 中使用的字符串表示。
    pub const fn as_str(self) -> &'static str {
        match self {
            ToolKind::Function => "function",
            ToolKind::Custom => "custom",
            ToolKind::Web => "web",
            ToolKind::File => "file",
            ToolKind::Shell => "shell",
            ToolKind::Skills => "skills",
            ToolKind::Code => "code",
            ToolKind::Mcp => "mcp",
            ToolKind::OpenApi => "openapi",
            ToolKind::Unknown => "unknown",
        }
    }
}

impl ContextProviderKind {
    /// 返回 YAML/JSON 中使用的字符串表示。
    pub const fn as_str(self) -> &'static str {
        match self {
            ContextProviderKind::Memory => "memory",
            ContextProviderKind::Skills => "skills",
            ContextProviderKind::Mcp => "mcp",
            ContextProviderKind::Workspace => "workspace",
            ContextProviderKind::Knowledge => "knowledge",
            ContextProviderKind::Wiki => "wiki",
            ContextProviderKind::History => "history",
            ContextProviderKind::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for ToolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for ContextProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
