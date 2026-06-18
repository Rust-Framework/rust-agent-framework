# 内置工具概览

RAF 提供 14 个开箱即用的内置工具，涵盖文件操作、目录管理、命令执行和 Skill 系统。所有工具均遵循 `ITool` 接口，支持工作区范围约束和人工审批。

## 工具列表

| 工具 | 模块 | 用途 |
|------|------|------|
| `read_file` | `tools::ReadFile` | 读取文件内容，支持行号范围 |
| `write_file` | `tools::WriteFile` | 创建或覆盖文件 |
| `edit_file` | `tools::EditFile` | 按精确字符串替换编辑文件 |
| `list_files` | `tools::ListFiles` | 列出目录内容 |
| `inspect_file` | `tools::InspectFile` | 检查文件元数据（大小、类型等） |
| `make_directory` | `tools::MakeDirectory` | 创建目录（含父目录） |
| `remove_path` | `tools::RemovePath` | 删除文件或空目录 |
| `move_file` | `tools::MoveFile` | 移动或重命名文件 |
| `find_files` | `tools::FindFiles` | 按 glob 模式搜索文件 |
| `search_file` | `tools::SearchFile` | 在文件内容中搜索正则表达式 |
| `run_command` | `tools::RunCommand` | 执行 shell 命令 |
| `load_skill` | `tools::LoadSkillTool` | 加载 Skill 定义 |
| `read_skill_resource` | `tools::ReadSkillResourceTool` | 读取 Skill 关联资源 |
| `run_skill_script` | `tools::RunSkillScriptTool` | 执行 Skill 脚本 |

## 注册工具

### 独立注册

```rust
use rust_agent_framework::tools::{ReadFile, WriteFile, ListFiles};

let agent = AgentBuilder::new("file-agent")
    .chat_client(client)
    .instructions("你是一个文件管理助手")
    .with_tool(ReadFile { scope: None })
    .with_tool(WriteFile { scope: None })
    .with_tool(ListFiles { scope: None })
    .build()?;
```

### 通过 WorkspaceContextProvider 批量注册

```rust
use rust_agent_core::{WorkspaceScope, ScopePolicy};
use rust_agent_framework::context_providers::WorkspaceContextProvider;
use rust_agent_framework::tools::*;

let scope = Arc::new(WorkspaceScope::new("/project", "my-project")
    .with_policy(ScopePolicy::AllowAll));

let workspace = WorkspaceContextProvider::new(scope)
    .add_tool(ReadFile { scope: None })
    .add_tool(WriteFile { scope: None })
    .add_tool(EditFile { scope: None })
    .add_tool(ListFiles { scope: None })
    .add_tool(InspectFile { scope: None })
    .add_tool(MakeDirectory { scope: None })
    .add_tool(RemovePath { scope: None })
    .add_tool(MoveFile { scope: None })
    .add_tool(FindFiles { scope: None })
    .add_tool(SearchFile { scope: None })
    .add_tool(RunCommand { scope: None, timeout_secs: None });

let agent = AgentBuilder::new("workspace-agent")
    .chat_client(client)
    .instructions("你是一个工作区管理助手")
    .add_context_provider(workspace)
    .build()?;
```

## 文件操作工具详解

### read_file

读取文件内容，支持分页读取。

```rust
ReadFile { scope: None }
```

**参数**：

| 参数 | 类型 | 说明 |
|------|------|------|
| `path` | `String` | 文件路径（相对或绝对） |
| `offset` | `Option<i64>` | 起始行号（1-based），默认 1 |
| `limit` | `Option<i64>` | 读取行数限制 |

**返回示例**：

```json
{
  "path": "src/main.rs",
  "content": "fn main() {\n    println!(\"...\");\n}",
  "total_lines": 42,
  "start_line": 1,
  "end_line": 42,
  "truncated": false,
  "scope": "inside_workspace"
}
```

**限制**：最大文件大小 512KB，单行最多显示 2000 字符（超长截断）。

### write_file

创建新文件或覆盖已存在的文件。

```rust
WriteFile { scope: None }
```

**参数**：

| 参数 | 类型 | 说明 |
|------|------|------|
| `path` | `String` | 目标文件路径 |
| `content` | `String` | 写入内容 |

**返回示例**：

```json
{
  "path": "output.txt",
  "bytes_written": 1024,
  "scope": "inside_workspace"
}
```

**限制**：内容最大 1MB，自动创建父目录。

### edit_file

按精确字符串匹配替换文件中的内容。

```rust
EditFile { scope: None }
```

**参数**：

| 参数 | 类型 | 说明 |
|------|------|------|
| `path` | `String` | 目标文件路径 |
| `old_string` | `String` | 要替换的原字符串（必须唯一匹配） |
| `new_string` | `String` | 替换后的新字符串 |

### list_files

列出指定路径下的文件和目录。

```rust
ListFiles { scope: None }
```

**参数**：

| 参数 | 类型 | 说明 |
|------|------|------|
| `path` | `String` | 目录路径 |

**返回示例**：

```json
{
  "path": "src",
  "entries": [
    {"name": "main.rs", "type": "file", "size": 1024},
    {"name": "utils", "type": "dir", "size": 0}
  ],
  "count": 2,
  "scope": "inside_workspace"
}
```

### run_command

执行 shell 命令并返回输出。

```rust
RunCommand { scope: None, timeout_secs: None }
```

**参数**：

| 参数 | 类型 | 说明 |
|------|------|------|
| `command` | `String` | 要执行的命令 |
| `working_dir` | `Option<String>` | 工作目录 |
| `timeout_secs` | `Option<u64>` | 超时时间（秒），默认 30 |

**返回示例**：

```json
{
  "stdout": "total 12\ndrwxr-xr-x  ...",
  "stderr": "",
  "exit_code": 0,
  "scope": "inside_workspace"
}
```

**限制**：输出最大 100KB，超时默认 30 秒。

## 工作区范围（WorkspaceScope）

所有文件操作工具都实现了 `IScopeTool`，通过 `WorkspaceContextProvider` 自动注入 `WorkspaceScope`。

```rust
pub struct WorkspaceScope {
    pub root: PathBuf,              // 工作区根目录
    pub name: String,               // 可读名称
    pub policy: ScopePolicy,        // 越界策略
    pub properties: HashMap<String, serde_json::Value>,
}

pub enum ScopePolicy {
    AllowAll,        // 开发模式：无限制
    ApproveOutside,  // 生产模式：越界需审批
    DenyOutside,     // 受限模式：禁止越界
}
```

当 `policy = ApproveOutside` 时，越界工具调用会先暂停，发出 `ToolApprovalRequest` 事件，待用户审批后恢复执行。

## 工具返回的 scope 字段

每个文件工具返回结果中都包含 `scope` 字段，值为：

| 值 | 含义 |
|----|------|
| `inside_workspace` | 路径在工作区内 |
| `outside_workspace` | 路径在工作区外 |
| `none` | 未配置 scope |

## 自定义工具

除了内置工具，你也可以轻松创建自定义工具：

```rust
use rust_agent_macros::tool;
use rust_agent_core::{ITool, ToolResult, AgentError};

#[tool(description = "Queries a database by ID.")]
struct DbQuery {
    connection_string: String,
}

impl DbQuery {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args { id: i64 }
        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| AgentError::ToolError(e.to_string()))?;

        // 执行数据库查询逻辑...
        // 返回成功或错误
        Ok(ToolResult::success(serde_json::json!({
            "id": args.id,
            "name": "example"
        })))
    }
}
```

## 下一步

内置工具熟悉后，请进入 **[第 2 章：核心架构](../02-core-architecture/INDEX.md)**，深入了解框架的分层设计和类型系统。
