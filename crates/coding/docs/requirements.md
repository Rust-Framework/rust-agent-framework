# TODO 待办事项应用 — 需求规格说明书

> **版本**: v1.0  
> **状态**: 已确认  
> **需求来源**: 用户初始需求 —— "实现一个简单的 TODO 待办事项应用，支持增删改查和标记完成"

---

## 1. 概述

### 1.1 产品定位
轻量级、单用户、命令行交互的 TODO 待办事项管理工具。用户通过终端对任务进行全生命周期管理（创建→查看→更新→完成→删除）。

### 1.2 核心能力
| 能力 | 说明 |
|------|------|
| **增** (Create) | 添加新的待办事项，包含标题和可选描述 |
| **删** (Delete) | 按 ID 删除已完成或废弃的待办事项 |
| **改** (Update) | 修改已有待办事项的标题、描述 |
| **查** (List/Get) | 列出所有事项 / 查看单个事项详情 |
| **标记完成** (Toggle) | 标记事项为"已完成"或重新打开为"待办" |

### 1.3 非目标（明确不做的）
- ❌ 不支持多用户 / 用户认证
- ❌ 不支持分类 / 标签 / 优先级
- ❌ 不支持截止日期 / 提醒
- ❌ 不支持持久化到外部数据库（仅本地文件存储）
- ❌ 不支持 Web / GUI 界面（仅 CLI）

---

## 2. 服务接口需求

> 由于本应用为 **CLI 应用**，无远程 API 服务，因此"服务接口"体现为 **内部核心数据层的 Rust Trait 接口**。后续架构设计将基于此接口进行实现。

### 2.1 核心接口定义：`TodoRepository`

```rust
/// 待办事项数据访问接口
#[async_trait]
pub trait TodoRepository: Send + Sync {
    /// 创建待办事项，返回生成的任务
    async fn create(&self, input: CreateTodoInput) -> Result<Todo>;

    /// 按 ID 获取单个待办事项
    async fn get(&self, id: TodoId) -> Result<Todo>;

    /// 获取全部待办事项列表
    async fn list(&self) -> Result<Vec<Todo>>;

    /// 更新待办事项（标题/描述）
    async fn update(&self, id: TodoId, input: UpdateTodoInput) -> Result<Todo>;

    /// 删除待办事项
    async fn delete(&self, id: TodoId) -> Result<()>;

    /// 切换完成状态（已完成 ↔ 未完成）
    async fn toggle(&self, id: TodoId) -> Result<Todo>;
}
```

### 2.2 数据模型定义

```rust
/// 待办事项 ID（采用 UUID v4）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TodoId(pub String);

/// 待办事项实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: TodoId,
    pub title: String,
    pub description: Option<String>,
    pub completed: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 创建待办事项的输入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTodoInput {
    pub title: String,
    pub description: Option<String>,
}

/// 更新待办事项的输入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTodoInput {
    pub title: Option<String>,
    pub description: Option<String>,
}
```

### 2.3 错误类型定义

```rust
/// 领域错误类型
#[derive(Debug, thiserror::Error)]
pub enum TodoError {
    #[error("待办事项未找到: {0}")]
    NotFound(TodoId),

    #[error("无效的标题: {0}")]
    InvalidTitle(String),

    #[error("数据持久化错误: {0}")]
    PersistenceError(#[from] std::io::Error),

    #[error("序列化错误: {0}")]
    SerializationError(#[from] serde_json::Error),
}
```

### 2.4 预期"调用"场景

| 场景 | 触发者 | 调用时机 | 频率 |
|------|--------|----------|------|
| `create` | CLI 命令处理器 | 用户输入 `add <title>` | 按需，低频 |
| `list` | CLI 命令处理器 | 用户输入 `list` | 按需，中频 |
| `get`  | CLI 命令处理器 | 用户输入 `get <id>` | 按需，低频 |
| `update` | CLI 命令处理器 | 用户输入 `update <id> --title xxx` | 按需，低频 |
| `delete` | CLI 命令处理器 | 用户输入 `delete <id>` | 按需，低频 |
| `toggle` | CLI 命令处理器 | 用户输入 `done <id>` 或 `undo <id>` | 按需，低频 |

### 2.5 预期结果

| 操作 | 成功响应 | 错误响应 | 状态码（CLI 退出码） |
|------|----------|----------|---------------------|
| `create` | 打印创建的 Todo JSON | 标题为空 → 打印错误信息 | 0 / 1 |
| `list`   | 打印 Todo 列表（表格或 JSON） | 无事项 → 打印"暂无待办事项" | 0 |
| `get`    | 打印单个 Todo JSON | ID 不存在 → "未找到" | 0 / 1 |
| `update` | 打印更新后的 Todo JSON | ID 不存在 → "未找到" | 0 / 1 |
| `delete` | 打印"已删除" | ID 不存在 → "未找到" | 0 / 1 |
| `toggle` | 打印状态切换后的 Todo JSON | ID 不存在 → "未找到" | 0 / 1 |

---

## 3. 应用界面需求 — CLI 交互设计

### 3.1 整体命令语法

```
todo [COMMAND] [ARGS] [OPTIONS]
```

支持以下子命令：

| 命令 | 别名 | 参数 | 说明 |
|------|------|------|------|
| `add <title>` | `a`, `create` | `[--desc, -d <description>]` | 创建待办事项 |
| `list` | `ls`, `all` | `[--all]` (默认) / `[--pending]` / `[--done]` | 列出事项 |
| `get <id>` | `show` | — | 查看单个事项详情 |
| `update <id>` | `edit`, `upd` | `[--title, -t <title>]` `[--desc, -d <description>]` | 更新事项 |
| `delete <id>` | `del`, `rm`, `remove` | — | 删除事项 |
| `done <id>` | `complete`, `finish` | — | 标记为已完成 |
| `undo <id>` | `reopen` | — | 重新打开（标记为未完成） |
| `help` | `-h`, `--help` | — | 显示帮助信息 |

### 3.2 用户交互场景与流程图

#### 场景 1：创建待办事项

```
$ todo add "买 groceries" --desc "牛奶、面包、鸡蛋"
✅ 已创建:
  ID:   a1b2c3d4-...
  标题: 买 groceries
  描述: 牛奶、面包、鸡蛋
  状态: ◻ 待办
  创建: 2025-01-15 10:30:00 UTC
```

**用户操作路径**:
1. 用户在终端输入 `todo add <title>` 命令
2. CLI 解析参数，验证 `title` 非空
3. 调用 `TodoRepository::create()` 生成 Todo 实体（含 UUID、时间戳）
4. 将实体持久化到本地 JSON 文件
5. 渲染成功输出（含格式化后的 Todo 信息）
6. 进程退出码为 0

**边界情况**:
- `title` 为空字符串 → 提示"标题不能为空"，退出码 1
- `title` 超过 200 字符 → 截断或提示"标题过长"
- `--desc` 可选，不提供则 `description = None`

#### 场景 2：列出待办事项

```
$ todo list
📋 待办事项列表 (共 3 项)

  #1  ◻ 买 groceries            2025-01-15 10:30
  #2  ◻ 学习 Rust 生命周期       2025-01-14 09:00
  #3  ☑ 完成需求文档             2025-01-13 16:20  ✅

$ todo list --pending
📋 待办事项 (未完成: 2 项)

  #1  ◻ 买 groceries            2025-01-15 10:30
  #2  ◻ 学习 Rust 生命周期       2025-01-14 09:00

$ todo list --done
📋 待办事项 (已完成: 1 项)

  #3  ☑ 完成需求文档             2025-01-13 16:20  ✅
```

**用户操作路径**:
1. 用户在终端输入 `todo list [--pending|--done]`
2. CLI 解析过滤参数
3. 调用 `TodoRepository::list()` 获取全部事项
4. 根据过滤参数筛选（默认不过滤，显示全部）
5. 按 `created_at` 降序排列
6. 渲染为易读的表格格式
7. 若无事项 → 输出"🎉 全部完成！暂无待办事项"

#### 场景 3：查看单个事项

```
$ todo get a1b2c3d4
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ID:      a1b2c3d4-e5f6-7890-abcd-ef1234567890
  标题:    买 groceries
  描述:    牛奶、面包、鸡蛋
  状态:    ◻ 待办
  创建:    2025-01-15 10:30:00 UTC
  更新:    2025-01-15 10:30:00 UTC
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

**用户操作路径**:
1. 用户输入 `todo get <id>` 或 `todo show <id>`
2. CLI 调用 `TodoRepository::get(id)` 查找
3. 若未找到 → 输出"❌ 未找到 ID 为 xxx 的待办事项"，退出码 1
4. 若找到 → 渲染详细信息，退出码 0

#### 场景 4：更新待办事项

```
$ todo update a1b2c3d4 --title "买 groceries 和生活用品"
✅ 已更新:
  ID:   a1b2c3d4-...
  标题: 买 groceries 和生活用品  ← 已变更
  描述: 牛奶、面包、鸡蛋
  状态: ◻ 待办
  更新: 2025-01-15 11:00:00 UTC
```

**用户操作路径**:
1. 用户输入 `todo update <id> [--title] [--desc]`
2. CLI 验证至少提供了一个修改字段
3. 调用 `TodoRepository::update(id, input)` 更新
4. 更新 `updated_at` 时间戳
5. 持久化保存
6. 输出更新后的 Todo 详情
7. ID 不存在 → 错误提示

**边界情况**:
- `--title` 和 `--desc` 都未提供 → 提示"请至少指定一个要修改的字段（--title 或 --desc）"
- 只更新 title 时 desc 保持不变（反之亦然）

#### 场景 5：删除待办事项

```
$ todo delete a1b2c3d4
🗑️ 已删除: "买 groceries 和生活用品"

$ todo delete nonexistent-id
❌ 未找到 ID 为 nonexistent-id 的待办事项
```

**用户操作路径**:
1. 用户输入 `todo delete <id>`
2. CLI 调用 `TodoRepository::delete(id)` 删除
3. 删除成功后打印删除确认 + 被删事项的标题
4. ID 不存在 → 友好错误提示

#### 场景 6：标记完成 / 重新打开

```
$ todo done b2c3d4e5
☑ 已标记为完成:
  ID:   b2c3d4e5-...
  标题: 学习 Rust 生命周期
  状态: ☑ 已完成  ✅
  更新: 2025-01-15 11:05:00 UTC

$ todo undo b2c3d4e5
◻ 已重新打开:
  ID:   b2c3d4e5-...
  标题: 学习 Rust 生命周期
  状态: ◻ 待办
  更新: 2025-01-15 11:06:00 UTC
```

**用户操作路径**:
1. 用户输入 `todo done <id>` 或 `todo undo <id>`
2. CLI 调用 `TodoRepository::toggle(id)`
3. 内部逻辑：`completed = !completed`
4. 更新 `updated_at` 时间戳
5. 持久化保存
6. 根据新状态输出不同风格的提示信息
7. `done` 一个已完成的 → 仍然标记为完成（幂等）  
   `undo` 一个未完成的 → 仍然标记为未完成（幂等）

### 3.3 全局行为

| 场景 | 行为 |
|------|------|
| 输入未知命令 | `todo unknown` → 打印帮助信息并提示"未知命令: unknown" |
| 输入 `-h` / `--help` | 打印详细的帮助文档，列出所有支持的命令 |
| 输入 `--version` | 打印应用版本号 |
| 输入不带参数 | 默认等同于 `todo list`（或打印帮助信息） |
| 数据文件损坏 | 提示"数据文件已损坏，请检查 ~/.todo/todos.json" |
| 数据文件不存在 | 首次运行时自动创建（空列表） |

---

## 4. 数据存储设计

### 4.1 存储方案
- **存储引擎**: 本地 JSON 文件
- **文件路径**: `$HOME/.todo/todos.json`
- **编码**: UTF-8

### 4.2 文件结构

```json
{
  "version": 1,
  "todos": [
    {
      "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "title": "买 groceries",
      "description": "牛奶、面包、鸡蛋",
      "completed": false,
      "created_at": "2025-01-15T10:30:00Z",
      "updated_at": "2025-01-15T10:30:00Z"
    }
  ]
}
```

### 4.3 并发安全
- 单用户 CLI 应用，不存在并发写入场景
- 每次写操作以"读取全量 → 修改 → 覆写全量"方式执行
- 写文件前先写入临时文件，写入成功后 `rename` 替换原文件（原子写入，防止断电/崩溃导致数据损坏）

---

## 5. 扩展性与非功能需求

### 5.1 性能要求

| 指标 | 目标值 | 说明 |
|------|--------|------|
| 命令启动时间 | < 100ms | 从输入命令到输出结果 |
| `list` 渲染时间（1000 条）| < 200ms | 含 JSON 反序列化 + 格式化输出 |
| 数据文件大小限制 | ≤ 10MB | 约 5 万条记录，超过应归档 |
| 内存占用 | < 50MB | 全量加载场景 |

### 5.2 风险分析

| 风险类别 | 风险描述 | 影响 | 缓解措施 |
|----------|----------|------|----------|
| **数据完整性风险** | 写入过程中程序崩溃 → JSON 文件损坏 | 丢失全部数据 | 采用"写临时文件→rename"原子写入策略 |
| **路径兼容性风险** | `$HOME` 在不同 OS 下行为不同 | 数据找不到 | 使用 `dirs` crate 跨平台获取家目录 |
| **大文件风险** | 数据量增长导致全量读写性能下降 | 用户体验下降 | 当前阶段可接受，后续可迁移至 SQLite |
| **编码风险** | 标题包含非 UTF-8 字符 | 序列化失败 | Rust 字符串始终为 UTF-8，风险低 |
| **用户误操作风险** | 误 `delete` 不可恢复 | 数据丢失 | 提供删除确认提示（可选） |

### 5.3 架构可扩展性（前瞻）

虽然当前需求为简单 TODO，但架构上应预留以下扩展点，以便后续迭代：

| 扩展方向 | 预留设计 |
|----------|----------|
| 多用户支持 | `TodoRepository` 接口抽象，可替换为数据库实现 |
| 持久化升级（SQLite/PostgreSQL）| 通过 Trait 接口隔离存储层 |
| 标签/分类/优先级 | `CreateTodoInput` 设计为 struct，可加字段 |
| 截止日期 | Todo 实体预留 `due_at: Option<DateTime<Utc>>` 字段（当前暂不实现） |
| Web API 层 | CLI 层的 `Command` 结构体可被复用为 API 处理器的逻辑 |
| 搜索/过滤 | `list()` 方法签名支持传入 `Filter` 参数（扩展性设计） |

---

## 6. 项目结构规划（建议）

```
todo-app/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI 入口，解析参数，派发命令
│   ├── cli.rs               # CLI 命令定义（使用 clap 或手写解析）
│   ├── model.rs             # 数据模型（Todo, TodoId, 输入/输出 DTO）
│   ├── repository.rs        # TodoRepository trait 定义
│   ├── file_repository.rs   # 基于 JSON 文件的 Repository 实现
│   ├── error.rs             # 错误类型定义
│   └── formatter.rs         # 输出格式化（表格/JSON/彩色）
├── tests/
│   ├── integration_test.rs  # 集成测试：全流程增删改查
│   └── repository_test.rs   # Repository 单元测试（使用 tempfile）
└── docs/
    └── requirements.md      # 本需求文档
```

---

## 7. 验收标准（用户故事维度）

作为用户，我期望：

1. ✅ **创建任务**: 输入 `todo add "任务名"` 后，能在列表中看到新任务
2. ✅ **查看列表**: 输入 `todo list` 能看到所有任务，已完成和未完成状态清晰可辨
3. ✅ **查看详情**: 输入 `todo get <id>` 能看到某任务的完整信息
4. ✅ **编辑任务**: 输入 `todo update <id> --title "新标题"` 能修改标题
5. ✅ **完成任务**: 输入 `todo done <id>` 后，该任务显示为已完成
6. ✅ **重开任务**: 输入 `todo undo <id>` 后，该任务恢复为未完成
7. ✅ **删除任务**: 输入 `todo delete <id>` 后，该任务从列表中消失
8. ✅ **帮助信息**: 输入 `todo --help` 能获得完整的命令说明
9. ✅ **错误提示**: 操作不存在的 ID 时，给出清晰的错误提示
10. ✅ **数据持久化**: 退出程序后重新启动，数据不会丢失

---

*本文档为第 1 阶段需求分析的输出物，经 HITL 确认后进入第 2 阶段（测试驱动设计）。*
