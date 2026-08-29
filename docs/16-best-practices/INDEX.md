# 第十六章 最佳实践

前面十五章讲完了「是什么」与「怎么用」。最后一章聚焦「怎么用得好」——如何组织一个真正落地的 rust-agent-framework 项目、单 Agent 与多 Agent 该如何设计、哪些坑会让你的流式输出和工具调用跑偏、性能优化的着力点在哪，以及声明白方式配置与 `AgentBuilder` 该如何取舍。这是全书最贴近实战的一章。

## 本章小节

| 小节 | 内容 |
|------|------|
| [项目组织与 Crates](project-structure.md) | workspace 依赖、crate 顺序与 feature 选择 |
| [Agent 设计指南](agent-design.md) | 单 Agent vs 多 Agent、记忆设计与工具设计 |
| [常见陷阱与排查](common-pitfalls.md) | 流式、工具注册、会话生命周期的坑 |
| [性能调优](performance-tuning.md) | 流式处理、缓存与 Provider 复用 |
| [声明式 vs AgentBuilder](declarative-vs-builder.md) | 何时用声明式配置、何时用程序化构建器 |
| [多智能体最佳实践](multi-agent-best-practices.md) | 编排模式选择与检查点策略 |

## 学习目标

读完本章，你应能：

- 搭出一个最小可运行的 rust-agent-framework 集成项目骨架，并正确选择所需 crate 与 feature
- 根据业务需求判断该用单 Agent 还是多 Agent，设计出边界清晰、可测试的 Agent 与工具
- 遇到「流式没输出」「工具没注册」「会话状态丢失」时快速定位并修复
- 说出至少 4 处框架已有的性能优化点，避免在错误的地方自己造轮子
- 在声明式配置（`AgentSchema` v1.0）与 `AgentBuilder` 之间做出合理取舍
- 根据编排水模式选出正确的 `*Workflow`，并为长流程设计检查点

## 下一步

全书完。从 [项目组织与 Crates](project-structure.md) 开始最后一程。