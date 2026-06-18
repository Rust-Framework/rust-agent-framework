# 第 7 章：人机协同与审批

在现实世界的 AI Agent 部署中，让 Agent 完全自主地执行所有操作往往是不安全或不可取的。生产环境的 Agent 需要对敏感操作（如删除文件、执行命令、访问工作区外的资源）引入人工审核环节。RAF 通过 `ApprovalRequiredTool` + `ToolApprovalRequest`/`ToolApprovalResponse` + `AwaitingApproval` 完成原因为核心，构建了一套完整的人机协同（Human-In-The-Loop，HITL）审批机制。

本章从设计理念开始，逐步深入审批流的完整链路、API 细节，以及中断恢复与取消机制。

| 小节 | 标题 |
|------|------|
| [7.1](hitl-overview.md) | 人机协同（HITL）概述 |
| [7.2](approval-flow.md) | 审批流完整链路 |
| [7.3](tool-approval-api.md) | ToolApprovalRequest/Response API |
| [7.4](resume-cancel.md) | 中断恢复与取消机制 |

## 快速导航

- **设计决策**：为什么审批逻辑放在 `FunctionInvokingChatClient` 装饰器层而非 Agent 层？参见 [7.1 — 设计理念](hitl-overview.md)。
- **端到端流程**：从 LLM 产出工具调用到用户批准再到工具执行的完整时序图，参见 [7.2 — 审批流完整链路](approval-flow.md)。
- **API 参考**：`ToolApprovalRequest` 和 `ToolApprovalResponse` 的字段说明与构建方式，参见 [7.3 — API](tool-approval-api.md)。
- **恢复执行**：审批暂停后如何重建消息上下文并继续执行，参见 [7.4 — 中断恢复](resume-cancel.md)。

## 核心类型速览

| 类型 | 所在 Crate | 用途 |
|------|-----------|------|
| `ApprovalRequiredTool` | `rust_agent_core::tool` | 包装 `ITool`，标记为需要审批 |
| `ToolApprovalResponse` | `rust_agent_core::tool` | 调用者对审批请求的响应 |
| `AgentResponseUpdate::ToolApprovalRequest` | `rust_agent_core::message` | 审批请求事件（流式管道内） |
| `FinishReason::AwaitingApproval` | `rust_agent_core::types` | 流暂停信号 |
| `AgentRunOptions.tool_approval_responses` | `rust_agent_core::run_options` | 恢复执行时携带的审批决定 |
| `AgentRunOptions.cancelled` | `rust_agent_core::run_options` | 取消标志（`Arc<AtomicBool>`） |

---

## 上一步

← [第 6 章：会话管理](../06-sessions/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[工作区管理](../08-workspace-management/scope-overview.md)** 以深入工作区安全边界，理解 WorkspaceScope、路径守卫和跨范围审批集成。
