# 第 8 章：工作区管理

工作区管理是 Agent 安全体系的核心组件。通过定义 **工作区范围（WorkspaceScope）** 和 **越界策略（ScopePolicy）**，RAF 能够精细控制 Agent 的文件系统和命令执行权限边界。本章深入解析 `WorkspaceScope` 的设计、`WorkspaceContextProvider` 的工具注入机制、`path_guard.rs` 的路径防护逻辑，以及跨范围审批的端到端集成。

| 小节 | 标题 |
|------|------|
| [8.1](scope-overview.md) | WorkspaceScope 工作区范围 |
| [8.2](workspace-context-provider.md) | WorkspaceContextProvider |
| [8.3](path-guard.md) | 路径守卫与跨范围检测 |
| [8.4](cross-scope-approval.md) | 跨范围审批集成 |
| [8.5](declarative-workspace.md) | 声明式工作区配置与工具联动 |

## 快速导航

- **了解工作区概念与策略**：参见 [8.1 — WorkspaceScope](scope-overview.md)。
- **如何将工作区感知注入 Agent**：参见 [8.2 — WorkspaceContextProvider](workspace-context-provider.md)。
- **路径如何被安全解析和检测**：参见 [8.3 — 路径守卫](path-guard.md)。
- **跨范围操作如何触发审批**：参见 [8.4 — 跨范围审批集成](cross-scope-approval.md)。

## 核心类型速览

| 类型 | 所在 Crate | 用途 |
|------|-----------|------|
| `WorkspaceScope` | `rust_agent_core::workspace` | 定义工作区根路径、名称、策略和属性 |
| `ScopePolicy` | `rust_agent_core::workspace` | 越界策略枚举：`AllowAll`、`ApproveOutside`、`DenyOutside` |
| `IScopeTool` | `rust_agent_core::workspace` | 可感知工作区范围的工具 trait |
| `WorkspaceContextProvider` | `rust_agent_framework::context_providers::workspace` | 工作区管理的 `IContextProvider` 实现 |
| `resolve_safe()` / `resolve_safe_new()` | `rust_agent_framework::tools::path_guard` | 安全路径解析函数 |
| `ScopeStatus` | `rust_agent_framework::tools::path_guard` | 路径范围检测结果枚举 |

---

## 上一步

← [第 7 章：人机协同与审批](../07-hitl-approval/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[ChatClient 管道](../09-chat-client-pipeline/decorator-pattern.md)** 以解析 ChatClient 装饰器管道架构，理解工具调用循环和流式处理链路。
