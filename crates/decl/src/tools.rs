use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schema::PropertySchema;

/// 工具类别枚举。`kind` 是分类命名空间，`name` 选定具体工具。
///
/// # 类别一览
///
/// | kind       | name 示例                          | 实例化                         |
/// |------------|-----------------------------------|-------------------------------|
/// | `function` | `echo`, `add`                     | `with_tool()` 注册的用户工具    |
/// | `custom`   | `my_plugin`                       | `register_factory()` 注册      |
/// | `web`      | `web_search`, `web_fetch`         | `WebSearch`, `WebFetch`       |
/// | `file`     | `read_file`, `write_file`, ...    | 10 个文件系统工具               |
/// | `shell`    | `run_command`                     | `RunCommand` (平台感知)        |
/// | `skills`   | `load_skill`, `read_skill_resource` | `LoadSkillTool`, `ReadSkillResourceTool` |
/// | `code`     | `code_interpreter`                | 沙箱代码执行                    |
/// | `mcp`      | —                                 | MCP 远程工具                   |
/// | `openapi`  | —                                 | OpenAPI 规范工具               |
///
/// # 关于 description
///
/// `web`、`file`、`shell`、`skills`、`code` 类别的工具 description 已内置在 `#[tool]` 宏中，
/// YAML 只需写 `name` 即可，不需要重复 description。
/// `function` 和 `custom` 类别由用户声明，description 在 YAML 中提供。
///
/// # YAML 示例
///
/// ```yaml
/// tools:
///   - kind: web
///     name: web_search
///   - kind: file
///     name: read_file
///   - kind: function
///     name: echo
///     description: Echoes back the input text
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDecl {
    /// 用户注册的函数工具。
    ///
    /// 由 `DeclAgentBuilder::with_tool()` 注册，在 YAML 中声明
    /// `kind: function` + `name` + `description`。
    #[serde(rename = "function")]
    Function {
        /// 函数名称 — 与 `with_tool()` 注册时的键匹配。
        name: String,
        /// 向 LLM 暴露的功能说明。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        /// 参数 JSON Schema（可选）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameters: Option<PropertySchema>,
        /// inputSchema 属性到参数的映射。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bindings: Vec<ToolBinding>,
    },

    /// 工厂注册的自定义工具。
    ///
    /// 通过 `ToolResolver::register_factory(name, factory)` 动态注册。
    #[serde(rename = "custom")]
    Custom {
        /// 工具名称 — 与注册工厂时的键匹配。
        name: String,
        /// 向 LLM 暴露的功能说明。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        /// 透传给工厂的任意配置。
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        config: HashMap<String, serde_json::Value>,
    },

    /// Web 工具：`web_search`、`web_fetch`。
    ///
    /// description 由 `#[tool]` 宏内建，YAML 无需提供。
    /// 省略 name 时注册全部 web 工具。
    #[serde(rename = "web")]
    Web {
        /// 工具名 — `"web_search"` 或 `"web_fetch"`。省略时注册全部。
        #[serde(default)]
        name: Option<String>,
        /// 可选描述（覆盖内建描述）。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
    },

    /// 文件系统工具：`read_file`、`write_file` 等 11 个。
    ///
    /// description 由 `#[tool]` 宏内建，YAML 无需提供。
    /// 省略 name 时注册全部文件系统工具。
    #[serde(rename = "file")]
    File {
        /// 工具名 — 如 `"read_file"`、`"write_file"`。省略时注册全部。
        #[serde(default)]
        name: Option<String>,
        /// 可选描述（覆盖内建描述）。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
    },

    /// Shell 命令执行工具：`run_command`。
    ///
    /// description 由 `RunCommand` 内部生成（平台感知），YAML 无需提供。
    #[serde(rename = "shell")]
    Shell {
        /// 工具名 — `"run_command"`。
        #[serde(default)]
        name: Option<String>,
        /// 可选描述（覆盖内建描述）。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
    },

    /// 代码执行工具：`code_interpreter`。
    ///
    /// description 由 `#[tool]` 宏内建，YAML 无需提供。
    /// 省略 name 时注册全部代码工具。
    #[serde(rename = "code")]
    Code {
        /// 工具名 — `"code_interpreter"`。省略时注册全部。
        #[serde(default)]
        name: Option<String>,
        /// 可选描述（覆盖内建描述）。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
    },

    /// MCP（Model Context Protocol）远程工具。
    #[serde(rename = "mcp")]
    Mcp {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_mode: Option<String>,
    },

    /// OpenAPI 3.x 规范驱动工具。
    #[serde(rename = "openapi")]
    OpenApi {
        name: String,
        #[serde(rename = "specUrl")]
        spec_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_id: Option<String>,
    },
}

/// 从 inputSchema 属性到工具参数的绑定。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolBinding {
    pub name: String,
    pub input: String,
}

impl ToolDecl {
    /// 获取工具名称（所有带 name 字段的变体）。
    /// Web/File/Code 的 name 为 Option，此时返回 None。
    pub fn name(&self) -> Option<&str> {
        match self {
            ToolDecl::Function { name, .. } => Some(name),
            ToolDecl::Custom { name, .. } => Some(name),
            ToolDecl::Web { name, .. } => name.as_deref(),
            ToolDecl::File { name, .. } => name.as_deref(),
            ToolDecl::Shell { name, .. } => name.as_deref(),
            ToolDecl::Code { name, .. } => name.as_deref(),
            ToolDecl::Mcp { name, .. } => Some(name),
            ToolDecl::OpenApi { name, .. } => Some(name),
        }
    }

    /// 获取工具类别字符串。
    pub fn kind_str(&self) -> &'static str {
        match self {
            ToolDecl::Function { .. } => "function",
            ToolDecl::Custom { .. } => "custom",
            ToolDecl::Web { .. } => "web",
            ToolDecl::File { .. } => "file",
            ToolDecl::Shell { .. } => "shell",
            ToolDecl::Code { .. } => "code",
            ToolDecl::Mcp { .. } => "mcp",
            ToolDecl::OpenApi { .. } => "openapi",
        }
    }

    /// 创建 function 声明。
    pub fn function(name: impl Into<String>, description: impl Into<String>) -> Self {
        ToolDecl::Function {
            name: name.into(),
            description: description.into(),
            parameters: None,
            bindings: Vec::new(),
        }
    }

    /// 创建 custom 声明。
    pub fn custom(name: impl Into<String>) -> Self {
        ToolDecl::Custom {
            name: name.into(),
            description: String::new(),
            config: HashMap::new(),
        }
    }

    /// 创建 web 声明。
    pub fn web(name: impl Into<String>) -> Self {
        ToolDecl::Web {
            name: Some(name.into()),
            description: String::new(),
        }
    }

    /// 创建 file 声明。
    pub fn file(name: impl Into<String>) -> Self {
        ToolDecl::File {
            name: Some(name.into()),
            description: String::new(),
        }
    }

    /// 创建 shell 声明。
    pub fn shell(name: impl Into<String>) -> Self {
        ToolDecl::Shell {
            name: Some(name.into()),
            description: String::new(),
        }
    }

    /// 创建 MCP 声明。
    pub fn mcp(name: impl Into<String>, server_url: impl Into<String>, tool_name: impl Into<String>) -> Self {
        ToolDecl::Mcp {
            name: name.into(),
            server_url: Some(server_url.into()),
            tool_name: Some(tool_name.into()),
            approval_mode: None,
        }
    }

    /// 是否需要展开为该分类下全部工具（无 name 的 web/file/code）。
    pub fn needs_expansion(&self) -> bool {
        match self {
            ToolDecl::Web { name, .. } => name.is_none(),
            ToolDecl::File { name, .. } => name.is_none(),
            ToolDecl::Shell { name, .. } => name.is_none(),
            ToolDecl::Code { name, .. } => name.is_none(),
            _ => false,
        }
    }
}
