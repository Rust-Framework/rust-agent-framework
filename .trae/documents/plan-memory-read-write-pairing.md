# Plan: SkillMemoryContextProvider 记忆读写配对重构（修订版）

## 摘要

重构 `SkillMemoryContextProvider`，实现 `SKILL.md`（读）和 `AGENT.md`（写）的读写配对记忆体系。新增 `MemoryAgent` 子代理在 `on_invoked` 中异步运行，负责每轮对话后增量沉淀和维护记忆。

***

## 1. 当前状态

```
memory/
├── skill_memory_context_provider.rs  ← on_invoked 空实现，tools 委托 AgentSkillsProvider
└── skill/
    ├── SKILL.md                      ← 读取导航
    ├── memories/
    │   ├── SOUL.md / PREFERENCE.md / USER.md / LESSON.md / RULES.md
    └── knowledge/
        ├── INDEX.md
        ├── rust/  （空目录）
        └── csharp/（空目录）
```

**阻塞点：**

| 问题                          | 根因                                                                                        |
| --------------------------- | ----------------------------------------------------------------------------------------- |
| `read_skill_resource` 可能不注册 | `AgentSkillsProvider::build_tools()` 依赖 `has_resources()`，后者仅检查 `references/` 和 `assets/` |
| `on_invoked` 无写入逻辑          | 当前空实现                                                                                     |
| 读取和写入无配对                    | SKILL.md 只定义了读，写入没有入口                                                                     |

***

## 2. `has_resources()` 问题 — 解决方案

**决策：不在** **`AgentSkillsProvider`** **中修改。**

理由：`AgentSkillsProvider::build_tools()` 是通用技能系统，其 `has_resources()` 逻辑对其他类型技能仍有意义。修改它可能影响 `references/` 和 `assets/` 语义。

**做法：`SkillMemoryContextProvider`** **停止委托，自行构建工具。**

`SkillMemoryContextProvider::build_tools()` 不再调用 `self.skills_provider.build_tools()`，而是自己直接用 `AgentSkill` 构建 `load_skill` 和 `read_skill_resource` 两个工具，无条件注册 `read_skill_resource`。

***

## 3. MemoryAgent 设计 — 从 LLM 视角分析

### 3.1 核心权衡

| 维度   | 无 session（一次性）    | 有 session      |
| ---- | ----------------- | -------------- |
| 状态来源 | 文件系统（每次都读）        | 文件系统 + 自身对话历史  |
| 去重能力 | 读已有文件 → 发现重复 → 跳过 | 同时依赖历史决策       |
| 一致性  | 依赖文件内容，可能来回摇摆     | 历史约束，更一致       |
| 成本   | 每次读文件 + LLM 推理    | 额外 token（历史积累） |
| 腐化检测 | 每次读到过时内容 → 可标记    | 可跨回合追踪变化       |

### 3.2 决策：**不需要 Session，但需要多轮上下文**

**为什么不需要 Session：**

MemoryAgent 读取的文件系统本身就是持久状态。每次执行时：

1. 读取目标文件 → 获得已有内容的 ground truth
2. 与当前对话对比 → 判断增量
3. 写入合并后的内容 → 文件即为持久化决策结果

这天然保证：下次执行时读到的就是上次写入的结果，形成闭路循环，无需额外的 session 记忆。

**但需要多轮上下文：**

如果每次只传入"当前这一轮"的对话，MemoryAgent 看不到用户连续表达的累积效应。例如：

* 第 1 轮：用户说 "我喜欢简洁回复"

* 第 2 轮：用户说 "还是详细点好"

* 第 3 轮：用户说 "但要抓住重点"

如果 MemoryAgent 只看第 3 轮，可能写 `偏好的模糊偏好`。但如果看到 3 轮完整对话，就能写 `偏好：详细但突出重点 → 写入 PREFERENCE.md`。

**做法：传入完整的** **`request_messages`（不含 system），即本次调用的全部对话上下文。**

### 3.3 质量保证策略（内置于 AGENT.md 提示词）

| 策略       | AGENT.md 中的规则                                           |
| -------- | ------------------------------------------------------- |
| **去重**   | "写入前先读目标文件。如果已有完全相同的信息，不写入。"                            |
| **合并**   | "如果已有类似信息但内容更新，合并而非替换——保留旧内容，追加新内容。"                    |
| **简洁**   | "每条记忆不超过 3 行。只记录事实，不记录对话过程。"                            |
| **去噪**   | "如果本轮对话没有实质性的新信息，仅回复 'OK'，不写入任何文件。"                     |
| **防腐化**  | "如果发现已有记忆与当前对话矛盾，将旧内容标记为 `~~旧内容~~`，追加 `→ 新内容`，并注明变更日期。" |
| **渐进写入** | "每次写入不超过 500 字，避免一次性大量修改。"                              |

### 3.4 执行模型

```
on_invoked 被调用（异步，非阻塞主流程）
        │
        ▼
┌─────────────────────────────┐
│ 构造 MemoryAgent 输入        │
│                             │
│ system: AGENT.md 指令        │
│ user:   "[本轮对话上下文]"    │
│   │                         │
│   ├─ 用户消息列表             │
│   ├─ 助手回复列表             │
│   └─ （排除 system 消息）     │
│                             │
│ memory_root: {路径}          │
└──────────┬──────────────────┘
           ▼
┌─────────────────────────────┐
│ MemoryAgent 执行             │
│                             │
│ 1. 读 AGENT.md 指令          │
│ 2. 按需 read_file 读已有记忆  │
│ 3. 分析对话，判断是否值得写入  │
│ 4. write_file 增量更新       │
│ 5. 回复结果                  │
└──────────┬──────────────────┘
           ▼
      完成（消费 stream）
```

### 3.5 为何不每轮都运行

`on_invoked` 每轮都被调用。但 MemoryAgent 不应该每轮都执行 LLM 推理——成本太高。

**做法：引入** **`consolidation_interval`** **配置。**

* `consolidation_interval = 1` → 每轮执行（调试用）

* `consolidation_interval = 3` → 每 3 轮执行（推荐默认值）

* `consolidation_interval = 0` → 禁用 MemoryAgent

通过 session 的 `provider_state` 跟踪 `turn_count`，每 N 轮触发一次 MemoryAgent。

***

## 4. AGENT.md 设计草案

核心结构（完整内容在实现阶段编写）：

```
# Memory Agent — 记忆沉淀

你是 MemoryAgent，专责在每轮对话后整理和沉淀有价值的信息。

## 一、记忆区映射

| 对话中出现什么 | 写入哪个文件 |
|--------------|------------|
| 用户表达的身份、角色、背景 | memories/USER.md |
| 用户明确设定的偏好 | memories/PREFERENCE.md |
| 你犯错被纠正 | memories/LESSON.md |
| 用户给你定的规则 | memories/RULES.md |
| 专业知识、技术细节 | knowledge/{主题}/INDEX.md → 具体章节 |

## 二、工作流程

1. 分析本轮对话上下文
2. 如果无实质性新信息 → 回复 OK，不写文件
3. 确定目标文件 → read_file 读取已有内容
4. 判断是新增、合并还是无需操作
5. write_file 增量更新

## 三、质量规则
（去重、合并、简洁、防腐化、渐进写入）

## 四、输出格式
- 无更新：OK
- 有更新：简述更新了哪些文件
```

***

## 5. SKILL.md 补充

末尾追加一节：

```markdown
## 五、读写配对

读取与写入使用相同的目录结构：
- 主代理通过 `load_skill` / `read_skill_resource` **读取**记忆
- MemoryAgent 在后台**写入**和**维护**记忆
- 你不需要关心写入——MemoryAgent 会自动处理
- 你只需要在需要回忆时按照本文件指引读取
```

***

## 6. SkillMemoryContextProvider 结构变更

```rust
pub struct SkillMemoryContextProvider {
    enabled: bool,
    /// 记忆技能目录路径（用于 MemoryAgent 文件读写）
    memory_dir: PathBuf,
    /// 从 memory_dir 加载的 AgentSkill（用于构建 tools）
    skill: Option<AgentSkill>,
    /// MemoryAgent 使用的 LLM chat_client（可选）
    memory_agent_client: Option<Arc<dyn IChatClient>>,
    /// MemoryAgent 执行间隔（每 N 轮执行一次）
    consolidation_interval: usize,
}
```

方法变更：

* `new()` → 接受 `memory_dir`

* `with_memory_agent(client)` → builder 设置 MemoryAgent

* `with_consolidation_interval(n)` → builder 设置间隔

* `build_tools()` → 自行构建，不委托 AgentSkillsProvider

* `on_invoked()` → 检查 turn\_count，按间隔触发 MemoryAgent

***

## 7. 文件变更清单

| 文件                                        | 操作     | 说明                        |
| ----------------------------------------- | ------ | ------------------------- |
| `memory/skill/AGENT.md`                   | **新建** | MemoryAgent 系统提示词         |
| `memory/skill/SKILL.md`                   | **修改** | 追加"读写配对"章节                |
| `memory/skill_memory_context_provider.rs` | **重写** | 结构体 + tools + on\_invoked |

***

## 8. 验证

1. `cargo check -p rust-agent-framework` 零 warning
2. `read_skill_resource` 始终出现在 tools 列表中
3. SKILL.md 和 AGENT.md 中文件路径完全一致
4. MemoryAgent 的 on\_invoked 可通过 `consolidation_interval` 控制频率

