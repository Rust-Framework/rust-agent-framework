# RAF Skill 支持规划

> 基于 [Agent Skills 开放标准](https://agentskills.io/) + Microsoft Agent Framework 设计，为 RAF 提供轻量级技能基础设施。

***

## 一、Agent Skills 开放标准协议

### 1.1 协议概述

Agent Skills 是由 Anthropic 于 2025 年 12 月发起、由 [agentskills.io](https://agentskills.io/) 维护的开放标准。已被 Microsoft、OpenAI、GitHub、Google、VS Code、Cursor 等 26+ 平台采纳。核心定义：

```
expense-report/         # 技能目录
├── SKILL.md            # 必须 — YAML frontmatter + Markdown 指令
├── scripts/            # 可选 — 可执行脚本
├── references/         # 可选 — 参考文档
└── assets/             # 可选 — 模板 / 静态资源
```

### 1.2 SKILL.md 格式

```yaml
---
name: expense-report          # 必须：技能名称
description: File and validate expense reports.  # 必须：功能描述，1-1024 字符
license: Apache-2.0           # 可选
compatibility: Requires python3  # 可选
metadata:                     # 可选
  author: contoso-finance
  version: "2.1"
---

# 技能指令（Markdown）
1. 步骤一...
2. 步骤二...
```

### 1.3 渐进式披露（核心设计）

| 阶段            | 触发条件                            | 行为                         | Token 消耗  |
| ------------- | ------------------------------- | -------------------------- | --------- |
| **Advertise** | Agent 每次调用                      | 注入技能名称 + 描述到 system prompt | \~100/技能  |
| **Load**      | LLM 调用 `load_skill` 工具          | 加载完整 SKILL.md 指令           | <5000（建议） |
| **Read**      | LLM 调用 `read_skill_resource` 工具 | 读取 references/assets 文件    | 按需        |
| **Run**       | LLM 调用 `run_skill_script` 工具    | 执行 scripts/ 中脚本            | 按需        |

***

## 二、设计

### 2.1 三个核心类型

全部新增在 [framework/src/context\_providers/skills\_provider.rs](file:///d:/GitCode/RF/rust-agent-framework/crates/framework/src/context_providers/)。

#### AgentSkill — 技能对象

```rust
/// 技能元信息（从 SKILL.md frontmatter 解析）
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// AgentSkill — 一个技能对象，持有元数据 + 技能目录路径
///
/// 可对接多种来源：本地目录、数据库、远程 API 等。
pub struct AgentSkill {
    pub metadata: SkillMetadata,
    /// 技能根目录（本地路径，from_dir 时设置）
    root_dir: Option<PathBuf>,
    /// 自定义指令内容（直接指定时使用，优先于文件读取）
    instructions: Option<String>,
    /// 自定义资源内容表（resource_path → content）
    resources: HashMap<String, Vec<u8>>,
}

impl AgentSkill {
    /// 从目录加载（解析 SKILL.md frontmatter，后续按需读正文）
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Self>;

    /// 动态创建（用于数据库 / 远程等定制场景）
    pub fn dynamic(
        metadata: SkillMetadata,
        instructions: impl Into<String>,
    ) -> Self;

    /// 添加内联资源
    pub fn with_resource(mut self, path: &str, content: impl Into<Vec<u8>>) -> Self;
}
```

关键设计：`AgentSkill` 可 `from_dir()` 也可 `dynamic()`，支持从数据库等任意来源构造。

#### AgentSkillsProvider — 技能上下文提供程序

```rust
/// AgentSkillsProvider — IContextProvider 实现
///
/// 对标 MAF 的 AgentSkillsProvider (C#) / SkillsProvider (Python)。
pub struct AgentSkillsProvider {
    skills: Vec<AgentSkill>,
    script_runner: Option<Arc<dyn AgentSkillScriptRunner>>,
}

impl AgentSkillsProvider {
    pub fn new() -> Self;
    pub fn with_skill(mut self, skill: AgentSkill) -> Self;
    pub fn with_skills(mut self, skills: impl IntoIterator<Item = AgentSkill>) -> Self;
    pub fn with_script_runner(mut self, runner: Arc<dyn AgentSkillScriptRunner>) -> Self;
}

#[async_trait]
impl IContextProvider for AgentSkillsProvider {
    fn name(&self) -> &str { "AgentSkillsProvider" }

    async fn on_invoking(/* ... */) -> Result<ContextInjection> {
        // Advertise: 注入技能列表 + load_skill/read_skill_resource/run_skill_script 工具
        Ok(ContextInjection {
            instructions: Some(self.build_advertise_text()),
            tools: self.build_tools(),
            ..Default::default()
        })
    }
}
```

#### AgentSkillScriptRunner — 脚本执行器

```rust
/// 技能脚本执行器
#[async_trait]
pub trait AgentSkillScriptRunner: Send + Sync {
    /// 执行脚本，返回 stdout
    async fn run(
        &self,
        skill_name: &str,
        script_path: &Path,
        args: Option<Vec<String>>,
    ) -> Result<String>;
}

/// 默认子进程执行器（对标 MAF SubprocessScriptRunner）
pub struct SubprocessScriptRunner { /* ... */ }

impl AgentSkillScriptRunner for SubprocessScriptRunner { /* ... */ }
```

### 2.2 3 个内置工具

通过 `#[tool]` 宏定义，由 `AgentSkillsProvider.on_invoking()` 注入：

| 工具名                   | 参数                                                     | 功能                             | 对应阶段    |
| --------------------- | ------------------------------------------------------ | ------------------------------ | ------- |
| `load_skill`          | `skill_name: string`                                   | 返回指定技能的完整 SKILL.md Markdown 指令 | Stage 2 |
| `read_skill_resource` | `skill_name: string, resource_path: string`            | 读取技能目录内 references/assets 文件   | Stage 3 |
| `run_skill_script`    | `skill_name: string, script_path: string, args: array` | 执行技能目录内 scripts/ 脚本            | Stage 4 |

**仅注入必要的工具**：有资源时注入 `read_skill_resource`，有脚本时注入 `run_skill_script`，不无端注入。

### 2.3 使用方式

**编程式（不污染 AgentBuilder）：**

```rust
use rust_agent_framework::context_providers::AgentSkillsProvider;
use rust_agent_framework::context_providers::AgentSkill;

let skill = AgentSkill::from_dir("./skills/code-review")?;

let agent = AgentBuilder::new("assistant")
    .chat_client(client)
    .instructions("You are a helpful assistant.")
    .add_context_provider(           // ← 现有 API，无需修改
        AgentSkillsProvider::new()
            .with_skill(skill)
            .with_skill(AgentSkill::from_dir("./skills/git-ops")?)
    )
    .build()?;
```

**批量加载目录：**

```rust
let provider = AgentSkillsProvider::scan("./skills")?;
// 自动扫描 ./skills 下所有含 SKILL.md 的子目录
```

**动态技能（数据库等定制来源）：**

```rust
// 从数据库读取技能元信息
let row = db.query("SELECT name, description, instructions FROM skills WHERE id = ?", id)?;
let skill = AgentSkill::dynamic(
    SkillMetadata {
        name: row.name,
        description: row.description,
        ..Default::default()
    },
    row.instructions,
).with_resource("policy.pdf", row.policy_blob);
```

### 2.4 声明式支持

在 [decl/src/agent.rs](file:///d:/GitCode/RF/rust-agent-framework/crates/decl/src/agent.rs) 中，`ContextProviderDecl` 新增 `Skills` 变体：

```rust
pub enum ContextProviderDecl {
    InMemoryHistory,
    Memory { mode: String },
    /// 技能 — 指定技能名称，架构自动查找注册
    Skills {
        /// 技能名称列表。架构按以下顺序查找：
        ///   1. 配置的 skill_directories（AgentDecl 级配置）
        ///   2. 默认目录 ./skills
        names: Vec<String>,
    },
}
```

```json
{
  "id": "dev-assistant",
  "instructions": "You are a helpful coding assistant.",
  "model": { "provider": "openai", "model": "gpt-4o", "api_key": "$OPENAI_API_KEY" },
  "skill_directories": ["./company-skills", "./team-skills"],
  "context_providers": [
    { "type": "in_memory_history" },
    { "type": "skills", "names": ["code-review", "git-ops", "web-search"] }
  ]
}
```

Resolver 根据 `skill_directories` 自动扫描匹配 `names` 中列出的技能，构建 `AgentSkillsProvider`。

***

## 三、实现计划

### Phase 1：核心实现

| 任务                           | 文件                                                   | 内容                                                             |
| ---------------------------- | ---------------------------------------------------- | -------------------------------------------------------------- |
| 1.1 `AgentSkill`             | `framework/src/context_providers/skills_provider.rs` | from\_dir() / dynamic() / with\_resource()，YAML frontmatter 解析 |
| 1.2 `AgentSkillsProvider`    | 同上                                                   | IContextProvider 实现，advertise 文本生成，3 个工具                       |
| 1.3 `AgentSkillScriptRunner` | 同上                                                   | trait 定义 + SubprocessScriptRunner                              |
| 1.4 `scan()`                 | 同上                                                   | 批量扫描目录下所有 SKILL.md                                             |
| 1.5 测试                       | 同文件                                                  | frontmatter 解析、advertise 输出、工具功能                               |

**验证**：`cargo check -p rust-agent-framework && cargo test -p rust-agent-framework`

### Phase 2：声明式集成 + 示例

| 任务                                | 文件                                     | 内容                                      |
| --------------------------------- | -------------------------------------- | --------------------------------------- |
| 2.1 `ContextProviderDecl::Skills` | `decl/src/agent.rs`                    | 新增变体 + `AgentDecl.skill_directories` 字段 |
| 2.2 Resolver 解析                   | `decl/src/resolver.rs`                 | 按名称 + 目录自动匹配技能                          |
| 2.3 示例技能                          | `examples/skills/code-review/SKILL.md` | 代码审查技能 + references/style-guide.md      |
| 2.4 示例技能                          | `examples/skills/git-ops/SKILL.md`     | Git 操作技能                                |

**验证**：`cargo check --workspace && cargo test --workspace`

***

## 四、文件变更

### 新建

| 文件                                                          | 说明                                                                                 |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `crates/framework/src/context_providers/skills_provider.rs` | AgentSkill + AgentSkillsProvider + AgentSkillScriptRunner + SubprocessScriptRunner |
| `examples/skills/code-review/SKILL.md`                      | 示例技能                                                                               |
| `examples/skills/code-review/references/rust-guidelines.md` | 示例参考文档                                                                             |
| `examples/skills/git-ops/SKILL.md`                          | 示例技能                                                                               |

### 修改

| 文件                                              | 变更                                                                      |
| ----------------------------------------------- | ----------------------------------------------------------------------- |
| `crates/framework/src/context_providers/mod.rs` | 新增 `pub mod skills_provider;`                                           |
| `crates/framework/src/lib.rs`                   | re-export `AgentSkillsProvider`, `AgentSkill`, `AgentSkillScriptRunner` |
| `crates/decl/src/agent.rs`                      | `ContextProviderDecl::Skills` + `AgentDecl.skill_directories`           |
| `crates/decl/src/resolver.rs`                   | 解析 `Skills` 声明，按名称匹配技能                                                  |

**不修改**：`core/`、`AgentBuilder`、`rhai/`、`macros/`。

***

## 五、设计决策

| 决策                                             | 理由                                               |
| ---------------------------------------------- | ------------------------------------------------ |
| **不引入 ISkill trait**                           | Agent Skills 协议核心是文件，不是代码抽象。MAF 也没有 ISkill trait |
| **AgentSkill 是 struct，提供 from\_dir + dynamic** | 覆盖文件系统和数据库两种场景，无需额外 trait 层次                     |
| **不修改 AgentBuilder**                           | 复用现有 `add_context_provider()`，零侵入                |
| **声明式只指定名称，架构自动查找**                            | 通过 `skill_directories` 配置搜索路径，自动匹配               |
| **3 个工具按需注入**                                  | 无资源不注入 read，无脚本不注入 run，减少 token 浪费               |

## 六、验证

1. `AgentSkill::from_dir()` 正确解析 SKILL.md frontmatter
2. `AgentSkill::dynamic()` 正确构造内存技能
3. `AgentSkillsProvider` advertise 文本格式正确
4. `load_skill` / `read_skill_resource` / `run_skill_script` 工具正常工作
5. 声明式 `{type: "skills", names: [...]}` 自动匹配并加载技能
6. `cargo check --workspace && cargo test --workspace && cargo clippy --workspace` 通过

