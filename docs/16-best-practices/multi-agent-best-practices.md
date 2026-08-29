# 多智能体最佳实践

> **本节目标**：根据协作形态选出正确的 `*Workflow` 编排模式，并为长流程设计检查点与断点续传。

## 1. 目标

读完本节，你应能：从需求出发（顺序 / 并发 / 票选 / 群聊 / 交接）选出对应的 `SequentialWorkflow`、`ConcurrentWorkflow`、`VoteWorkflow`、`GroupChatWorkflow`、`HandoffWorkflow`，并通过 `WorkflowBuilder` 组装、给长流程加检查点。

## 2. 核心概念

- **顺序（SequentialWorkflow）**：步骤有严格先后，前一步输出喂给后一步。
- **并发（ConcurrentWorkflow）**：多个子 Agent 并行推进，按 Join / Race 聚合结果。
- **投票（VoteWorkflow）**：多 Agent 各自给方案，聚合域选出最佳。
- **群聊（GroupChatWorkflow）**：多 Agent 在一轮对话内轮流发言，由引导者调度。
- **交接（HandoffWorkflow）**：按意图把会话从当前 Agent 移交给更合适的 Agent。
- **WorkflowBuilder**：编排的构建入口，负责把 Agent 组装成统一可跑的 `IAgent`（Workflow 本身实现 `IAgent` 门面）。
- **检查点（Checkpoints）**：长流程运行到某步时保存进度，允许断点续传而不必从头重跑。

选型口诀：**看「协作形态」，不看「Agent 数量」。**

## 3. 可运行示例代码片段

**（一）用 WorkflowBuilder 组装三种常用编排：**

```rust
use rust_agent_framework::{AgentBuilder, WorkflowBuilder};

fn agent(id: &str) -> rust_agent_framework::IAgent {
    AgentBuilder::new(id).instructions("你是一个专业助手。").build().unwrap()
}

// 顺序：翻译 → 润色
let seq = WorkflowBuilder::new()
    .sequential()
    .add_agent("translator", agent("translator"))
    .add_agent("polisher", agent("polisher"))
    .build()?;

// 并发：两个独立综述并行，Join 聚合
let conc = WorkflowBuilder::new()
    .concurrent()
    .add_agent("a", agent("a"))
    .add_agent("b", agent("b"))
    .build()?;

// 投票：三个评审各自打分，票选结论
let vote = WorkflowBuilder::new()
    .vote()
    .add_agent("reviewer_1", agent("reviewer_1"))
    .add_agent("reviewer_2", agent("reviewer_2"))
    .add_agent("reviewer_3", agent("reviewer_3"))
    .build()?;
```

**（二）为长流程设计检查点（断点续传）：**

```rust
use rust_agent_framework::WorkflowBuilder;

// 长流程按阶段划分，每个阶段结束落一个检查点
let pipeline = WorkflowBuilder::new()
    .sequential()
    .add_agent("stage_1_extract", agent("extract"))
    .add_agent("stage_2_transform", agent("transform"))
    .add_agent("stage_3_validate", agent("validate"))
    .with_checkpoint("stage_1_extract")   // 第 1 阶段完成后落点
    .with_checkpoint("stage_2_transform") // 第 2 阶段完成后落点
    .build()?;

// 若在第 3 阶段崩溃，可从最近的检查点续传，跳过已完成的 stage_1/2
let resumed = pipeline.resume_from_checkpoint().await?;
```

## 4. 注意事项 / 常见陷阱

| 维度 | ✅ 应该 | ⛔ 不应该 |
|------|--------|----------|
| 选型 | 按「协作形态」选（顺序/并发/票选/群聊/交接） | 一味堆 Agent，用复杂工作流解决简单顺序问题 |
| 并发 | Join 场景确保子结果可独立聚合 | 并发子任务共享可变状态 |
| 交接 | 交接桩基于会话上下文传递 | 交接丢失历史（`HandoffWorkflow` 靠会话沟通） |
| 检查点 | 长流程/昂贵步骤前后落点 | 每个微小步骤都落点，开销过大 |
| 统一门面 | 对外统一暴露 `IAgent` | 用面返所有编排细节，耦合调用方 |

**常见陷阱**：

- **把并发当万能**：子任务间有数据依赖时不能并发，须退回 `SequentialWorkflow`。
- **检查点采样率失衡**：过多检查点引入序列化/IO 开销；过少则崩溃后重算代价大。按「步骤代价」决定落点。
- **群聊/交接忽略会话**：这两类编排依托会话与上下文，临时构造无会话上下文会导致丢失。

## 5. 小结

- 按协作形态选 `*Workflow`；能用顺序就别上并发，能上并发的别用全局锁。
- 长流程用检查点断点续传，落点频率与步进代价匹配。
- 所有编排对外统一实现 `IAgent` 门面，调用方无需感知内部结构。

下一节：本章结束，回到 [第十六章索引](INDEX.md)