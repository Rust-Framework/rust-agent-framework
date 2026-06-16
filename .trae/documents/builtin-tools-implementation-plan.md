# 内置工具实现规划

## 一、概述

在 `crates/framework/src/tools/` 下实现 **13 个 AI 原生友好的内置工具**，覆盖 LLM 编码开发全流程：探索代码 → 定位文件 → 读取代码 → 搜索模式 → 精准编辑 → 运行测试 → 修复错误。

## 二、LLM 编码需求覆盖

```
探索项目结构 → list_files / inspect_file
定位文件     → find_files
读取源码     → read_file
搜索关键模式 → search_file
精准编辑     → edit_file / write_file
文件管理     → make_directory / remove_path / move_file
运行测试/lint → run_command
查阅文档     → web_search / web_fetch
```

| 编码环节 | 工具 | 覆盖 |
|---------|------|------|
| 列出目录 | `list_files` | ✅ |
| 文件元数据 | `inspect_file` | ✅ |
| 按模式找文件 | `find_files` | ✅ |
| 按内容搜索 | `search_file` | ✅ |
| 读取文件 | `read_file` | ✅ |
| 全量写入 | `write_file` | ✅ |
| 创建目录 | `make_directory` | ✅ |
| 删除路径 | `remove_path` | ✅ |
| 移动/重命名 | `move_file` | ✅ |
| 执行命令 | `run_command` | ✅ |
| 精准编辑 | `edit_file` | ✅ |
| 网络搜索 | `web_search` | ✅ |
| 网页抓取 | `web_fetch` | ✅ |

## 三、当前架构分析

### 3.1 已就绪

- **`ITool` trait**（`crates/core/src/tool.rs`）：`name()` / `description()` / `parameters()` / `execute(arguments) -> Result<String>`
- **`#[tool]` 宏**（`crates/macros/src/lib.rs`）：标注 `async fn` 自动生成 ITool 实现 + JSON Schema
- **`ToolRegistry`**：`HashMap<String, Arc<dyn ITool>>`，支持注册/查找/列表
- **`ToolLoopAgent`**：自动 tool-calling 循环，并行执行

### 3.2 当前缺失

- `crates/framework/src/tools/` 目录不存在
- 无内置工具实现
- 无 `regex`、`glob`、`walkdir` 等依赖

## 四、设计原则（AI 原生友好）

| 原则 | 说明 |
|------|------|
| **verb_noun 命名** | 除 `web_search`/`web_fetch`（训练数据强对齐、行业惯例）外全部 verb_noun |
| **消除魔法字符串** | 不设 `action` 枚举参数，每个工具单一职责 |
| **参数无重载** | 每个参数一个语义，不因其他参数值而改变含义 |
| **训练数据对齐** | `edit_file(old_str, new_str)` 对齐 Aider/Claude Computer Use 生态 |
| **统一返回结构** | `{"ok": true/false, "data": ..., "error": "..."}` |
| **输出保护** | 所有工具限制输出大小，防止 LLM 上下文溢出 |

## 五、具体实现

### 5.1 新增文件

#### 5.1.1 `crates/framework/src/tools/mod.rs`

```rust
pub mod read_file;
pub mod write_file;
pub mod edit_file;
pub mod list_files;
pub mod inspect_file;
pub mod make_directory;
pub mod remove_path;
pub mod move_file;
pub mod find_files;
pub mod search_file;
pub mod run_command;
pub mod web_search;
pub mod web_fetch;

pub use read_file::ReadFile;
pub use write_file::WriteFile;
pub use edit_file::EditFile;
pub use list_files::ListFiles;
pub use inspect_file::InspectFile;
pub use make_directory::MakeDirectory;
pub use remove_path::RemovePath;
pub use move_file::MoveFile;
pub use find_files::FindFiles;
pub use search_file::SearchFile;
pub use run_command::RunCommand;
pub use web_search::WebSearch;
pub use web_fetch::WebFetch;

use rust_agent_core::ToolRegistry;

/// 一次性注册所有内置工具
pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(ReadFile);
    registry.register(WriteFile);
    registry.register(EditFile);
    registry.register(ListFiles);
    registry.register(InspectFile);
    registry.register(MakeDirectory);
    registry.register(RemovePath);
    registry.register(MoveFile);
    registry.register(FindFiles);
    registry.register(SearchFile);
    registry.register(RunCommand);
    registry.register(WebSearch);
    registry.register(WebFetch);
}
```

#### 5.1.2 `read_file` — 读取文件内容

```rust
#[tool(description = "Reads a file from the local filesystem. Supports line range via offset/limit.")]
async fn read_file(
    #[param(desc = "Absolute path to the file")] path: String,
    #[param(desc = "Starting line number (1-based, optional)")] offset: Option<i64>,
    #[param(desc = "Maximum number of lines to read (optional)")] limit: Option<i64>,
) -> String { ... }
```

- 读取整个文件或指定行范围
- 过大文件自动截断（末尾加 `[truncated]` 标记）
- 返回值：`{"ok": true, "data": {"path": "...", "content": "...", "total_lines": N, "start_line": N, "end_line": N}}`

#### 5.1.3 `write_file` — 全量写入文件

```rust
#[tool(description = "Creates a new file or overwrites an existing file with the given content.")]
async fn write_file(
    #[param(desc = "Absolute path to the file")] path: String,
    #[param(desc = "Content to write to the file")] content: String,
) -> String { ... }
```

- 覆盖写入，不存在则创建
- 自动创建父目录

#### 5.1.4 `edit_file` — 精准查找替换编辑

```rust
#[tool(description = "Performs exact string replacement in an existing file. Provide old_str (the exact text to find) and new_str (the replacement). The old_str must uniquely match a contiguous block of lines in the file.")]
async fn edit_file(
    #[param(desc = "Absolute path to the file to edit")] path: String,
    #[param(desc = "Exact text to find in the file (must be unique and contiguous)")] old_str: String,
    #[param(desc = "Text to replace it with")] new_str: String,
) -> String { ... }
```

- 读取文件，查找 `old_str` 的首次出现
- **严格唯一性校验**：如果 `old_str` 在文件中不出现或出现多次，返回错误
- 替换为新内容，写回文件
- 返回值：`{"ok": true, "data": {"path": "...", "replaced": true}}`

> 这是源自 Aider / Claude Computer Use 的**事实标准**编辑原语。

#### 5.1.5 `list_files` — 列出目录内容

```rust
#[tool(description = "Lists files and directories at the given path. Returns name, type (file/dir/symlink), and size for each entry.")]
async fn list_files(
    #[param(desc = "Absolute path to the directory")] path: String,
) -> String { ... }
```

- 递归？不递归 —— 一次一层，对齐 LLM 逐步探索的思维模式
- 返回值：`{"ok": true, "data": {"path": "...", "entries": [{"name": "...", "type": "file|dir|symlink", "size": N}, ...]}}`

#### 5.1.6 `inspect_file` — 获取文件/目录元数据

```rust
#[tool(description = "Returns metadata about a file or directory: type, size in bytes, modification time, permissions.")]
async fn inspect_file(
    #[param(desc = "Absolute path to the file or directory")] path: String,
) -> String { ... }
```

- 使用 `std::fs::metadata()` + `symlink_metadata()`
- 返回值：`{"ok": true, "data": {"path": "...", "type": "file|dir|symlink", "size": N, "modified": "RFC3339", "readonly": bool}}`

#### 5.1.7 `make_directory` — 创建目录

```rust
#[tool(description = "Creates a directory and all parent directories if they don't exist (like mkdir -p).")]
async fn make_directory(
    #[param(desc = "Absolute path of the directory to create")] path: String,
) -> String { ... }
```

- `std::fs::create_dir_all()`

#### 5.1.8 `remove_path` — 删除文件或目录

```rust
#[tool(description = "Deletes a file or directory at the specified path.")]
async fn remove_path(
    #[param(desc = "Absolute path to the file or directory to delete")] path: String,
) -> String { ... }
```

- 文件用 `remove_file`，目录用 `remove_dir_all`

#### 5.1.9 `move_file` — 移动/重命名

```rust
#[tool(description = "Moves or renames a file or directory.")]
async fn move_file(
    #[param(desc = "Source absolute path")] from: String,
    #[param(desc = "Destination absolute path")] to: String,
) -> String { ... }
```

- `std::fs::rename()`

#### 5.1.10 `find_files` — 按 glob 模式查找文件

```rust
#[tool(description = "Finds files matching a glob pattern (e.g. '**/*.rs'). Returns matching file paths.")]
async fn find_files(
    #[param(desc = "Glob pattern (e.g. '**/*.rs', 'src/*.ts')")] pattern: String,
    #[param(desc = "Root directory to search from (optional, defaults to current working directory)")] directory: Option<String>,
) -> String { ... }
```

- 使用 `glob` crate
- 限制结果数（500 条）

#### 5.1.11 `search_file` — 按内容搜索文件

```rust
#[tool(description = "Searches file contents using a regex pattern. Returns matching lines with file path and line number.")]
async fn search_file(
    #[param(desc = "Regular expression pattern to search for")] pattern: String,
    #[param(desc = "Directory to search recursively")] directory: String,
    #[param(desc = "Glob pattern to filter files to include (e.g. '*.rs')")] include: Option<String>,
    #[param(desc = "Case insensitive search (default: false)")] case_insensitive: Option<bool>,
) -> String { ... }
```

- 使用 `regex` crate 编译模式
- 使用 `walkdir` 递归遍历文件
- 使用 `glob` crate 过滤文件
- 限制结果数（200 条）
- 返回值：`{"ok": true, "data": {"matches": [{"file": "...", "line": N, "content": "..."}, ...], "total": N}}`

#### 5.1.12 `run_command` — 执行系统命令

```rust
#[tool(description = "Executes a shell command and returns the output (stdout + stderr) and exit code.")]
async fn run_command(
    #[param(desc = "Shell command to execute")] command: String,
    #[param(desc = "Working directory for the command (optional, defaults to current)")] working_dir: Option<String>,
) -> String { ... }
```

- Windows: `cmd /c {command}`
- Unix: `sh -c {command}`
- 捕获 stdout + stderr
- 输出限制 100KB 截断
- 返回值：`{"ok": true, "data": {"stdout": "...", "stderr": "...", "exit_code": N}}`

#### 5.1.13 `web_search` — 网络搜索

```rust
#[tool(description = "Searches the web and returns a list of results with title, URL, and snippet.")]
async fn web_search(
    #[param(desc = "Search query")] query: String,
    #[param(desc = "Maximum number of results to return (default: 5)")] count: Option<i64>,
) -> String { ... }
```

- 使用 `reqwest` 调用 DuckDuckGo 搜索
- 返回值：`{"ok": true, "data": {"results": [{"title": "...", "url": "...", "snippet": "..."}, ...]}}`

> `tarzi` 在 crates.io 上不是确定性选择；采用 DuckDuckGo + `reqwest` 实现，无需 API Key。

#### 5.1.14 `web_fetch` — 网页抓取

```rust
#[tool(description = "Fetches content from a URL and returns it as plain text.")]
async fn web_fetch(
    #[param(desc = "The URL to fetch")] url: String,
) -> String { ... }
```

- `reqwest::get()`，超时 10 秒
- 基础 HTML → 纯文本转换
- 限制 50KB
- 返回值：`{"ok": true, "data": {"url": "...", "title": "...", "content": "..."}}`

### 5.2 修改文件

#### 5.2.1 `crates/framework/src/lib.rs`

在模块声明末尾添加：

```rust
pub mod tools;
```

#### 5.2.2 `crates/framework/Cargo.toml`

新增依赖：

```toml
regex = "1"
glob = "0.3"
walkdir = "2"
```

`reqwest` 和 `tokio` 已在 workspace 级别定义，无需重复添加。

## 六、设计决策总结

| # | 决策 | 理由 |
|---|------|------|
| 1 | 消除 `action` 参数 | LLM 两级分发出错率高，拆分为独立工具降低认知负担 |
| 2 | verb_noun 命名 | 对齐 LLM 自然语言思维模型 |
| 3 | `edit_file(old_str, new_str)` | Aider/Claude 生态事实标准，训练数据强对齐 |
| 4 | `search_file` + `find_files` 双工具 | 分别对应"按内容搜"和"按名称找"，语义互不混淆 |
| 5 | `run_command` 加 `working_dir` | LLM 高频需求：在特定目录下跑命令 |
| 6 | `web_search`/`web_fetch` 保留原名 | 训练数据出现频率高，改名反降识别率 |
| 7 | DuckDuckGo 作为搜索后端 | 无需 API Key，零配置可用 |
| 8 | 统一返回 `{"ok": bool, "data": ..., "error": "..."}` | LLM 可快速判断成功/失败，基于 error 调整重试 |

## 七、验证步骤

1. `cargo build -p rust-agent-framework` — 编译通过
2. `cargo test -p rust-agent-framework` — 单元测试通过
3. `cargo build -p rust-agent-cli` — CLI 可引入工具
4. 在 CLI 中注册工具，通过对话测试 tool-calling 流程
5. 测试异常场景：无效路径、权限不足、网络不可达、`old_str` 不唯一

## 八、文件清单汇总

```
crates/framework/Cargo.toml              [修改] 新增 regex/glob/walkdir 依赖
crates/framework/src/lib.rs              [修改] 新增 pub mod tools;
crates/framework/src/tools/mod.rs        [新增] 模块入口 + register_all()
crates/framework/src/tools/read_file.rs  [新增]
crates/framework/src/tools/write_file.rs [新增]
crates/framework/src/tools/edit_file.rs  [新增]
crates/framework/src/tools/list_files.rs [新增]
crates/framework/src/tools/inspect_file.rs[新增]
crates/framework/src/tools/make_directory.rs[新增]
crates/framework/src/tools/remove_path.rs[新增]
crates/framework/src/tools/move_file.rs  [新增]
crates/framework/src/tools/find_files.rs [新增]
crates/framework/src/tools/search_file.rs[新增]
crates/framework/src/tools/run_command.rs[新增]
crates/framework/src/tools/web_search.rs [新增]
crates/framework/src/tools/web_fetch.rs  [新增]
```
