//! 声明式上下文提供器配置——(kind, name) 二元组 + 可选 config。
//!
//! 对标 `ToolDecl` 的 tagged enum 模式，但更简化为 kind + name 的组合。
//! config 使用 HashMap 而非固定结构体，便于后续扩展更多提供器类型。
//!
//! ## 分类说明
//!
//! | kind       | name 示例         | 说明                        |
//! |------------|-------------------|-----------------------------|
//! | `memory`   | `skill-memory`    | 持久化跨会话记忆系统         |
//! | `skills`   | `antd-skill`      | 按需加载的技能文件（SKILL.md）|
//! | `mcp`      | `mymcp-server`    | MCP 远程工具服务器           |
//! | `workspace`| `default`         | 工作区根目录 + 策略配置      |
//! | `knowledge`| `my-rag`          | RAG 知识库检索              |
//! | `wiki`     | `my-wiki`         | Wiki 知识库                 |
//!
//! websearch 属于工具（tools → kind: web），history 属于内置默认（InMemoryHistoryProvider），
//! 均不在此处声明。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 声明式上下文提供器——(kind, name) 二元组 + 可选 config。
///
/// # YAML 示例
///
/// ```yaml
/// contexts:
///   - kind: memory
///     name: skill-memory
///     config:
///       directory: logs/memory
///       consolidationInterval: 1
///   - kind: skills
///     name: antd-skill
///     config:
///       directory: skills/antd-skill
///   - kind: workspace
///     name: default
///     config:
///       root: .
///       policy: read
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ContextProviderDecl {
    /// 记忆系统 — 目前仅支持 name: "skill-memory"
    #[serde(rename = "memory")]
    Memory {
        name: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
    /// 技能系统 — name 指定技能名称（对应 SKILL.md 目录名）
    #[serde(rename = "skills")]
    Skills {
        name: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
    /// MCP 服务器 — name 指定服务器标识
    #[serde(rename = "mcp")]
    Mcp {
        name: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
    /// 工作区 — name 指定工作区标识，config.root 为根目录，config.policy 为访问策略
    #[serde(rename = "workspace")]
    Workspace {
        name: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
    /// RAG 知识库 — name 指定知识库标识
    #[serde(rename = "knowledge")]
    Knowledge {
        name: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
    /// Wiki 知识库 — name 指定 Wiki 标识
    #[serde(rename = "wiki")]
    Wiki {
        name: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
}

impl ContextProviderDecl {
    /// 获取提供器名称。
    pub fn name(&self) -> &str {
        match self {
            ContextProviderDecl::Memory { name, .. } => name,
            ContextProviderDecl::Skills { name, .. } => name,
            ContextProviderDecl::Mcp { name, .. } => name,
            ContextProviderDecl::Workspace { name, .. } => name,
            ContextProviderDecl::Knowledge { name, .. } => name,
            ContextProviderDecl::Wiki { name, .. } => name,
        }
    }

    fn config_map(&self) -> &HashMap<String, serde_json::Value> {
        match self {
            ContextProviderDecl::Memory { config, .. } => config,
            ContextProviderDecl::Skills { config, .. } => config,
            ContextProviderDecl::Mcp { config, .. } => config,
            ContextProviderDecl::Workspace { config, .. } => config,
            ContextProviderDecl::Knowledge { config, .. } => config,
            ContextProviderDecl::Wiki { config, .. } => config,
        }
    }

    /// 获取提供器 kind 字符串（与 `IContextProvider::kind()` 对齐）。
    pub fn kind_str(&self) -> &'static str {
        match self {
            ContextProviderDecl::Memory { .. } => "memory",
            ContextProviderDecl::Skills { .. } => "skills",
            ContextProviderDecl::Mcp { .. } => "mcp",
            ContextProviderDecl::Workspace { .. } => "workspace",
            ContextProviderDecl::Knowledge { .. } => "knowledge",
            ContextProviderDecl::Wiki { .. } => "wiki",
        }
    }

    /// 获取配置中字符串值的辅助方法。
    pub fn get_config_str(&self, key: &str) -> Option<&str> {
        self.config_map().get(key).and_then(|v| v.as_str())
    }

    /// 获取配置中布尔值的辅助方法。
    pub fn get_config_bool(&self, key: &str) -> Option<bool> {
        self.config_map().get(key).and_then(|v| v.as_bool())
    }

    /// 获取配置中数值的辅助方法。
    pub fn get_config_u64(&self, key: &str) -> Option<u64> {
        self.config_map().get(key).and_then(|v| v.as_u64())
    }
}
