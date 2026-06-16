# 新建 `crates/wiki` 库 — 实施计划

## 摘要

从 `D:\Github\llm-wiki` 源码中提取核心 wiki 引擎逻辑，创建新的 `crates/wiki`（Cargo 包名 `rust-agent-wiki`）库 crate，同时移除所有 git、mcp、acp 相关代码。该库将作为 `rust-agent-framework` workspace 的一个新成员。

***

## 当前状态分析

### 源项目 (`D:\Github\llm-wiki`)

`llm-wiki` 是一个基于文件系统的 Wiki 引擎，支持：

* YAML frontmatter 解析与页面类型系统（JSON Schema 验证）

* Tantivy 全文搜索索引

* 类型化页面注册表（`SpaceTypeRegistry`）

* 概念图谱构建与社区检测（petgraph + Louvain）

* Wikilink 提取（`[[wikilinks]]` 与 CommonMark 链接）

* Markdown 页面 CRUD 操作

* 文件系统监控（`notify`）

* Git 版本控制、MCP/ACP 传输层（需排除）

### 目标项目 (`d:\GitCode\RF\rust-agent-framework`)

一个 Rust workspace，包含 12 个 crate，遵循以下规范：

* **包命名**：`rust-agent-<name>`（如 `rust-agent-core`）

* **目录命名**：简化的 `<name>`（如 `crates/core/`）

* **版本/依赖管理**：集中定义于根 `Cargo.toml` 的 `[workspace.package]` 和 `[workspace.dependencies]` 中，各 crate 使用 `workspace = true` 引用

* **模块组织**：按 `src/` 下子目录划分模块（如 `ops/`），子目录内使用 `mod.rs`

* **模式**：edition = "2021"，license = "MIT"

***

## 提议变更

### 1. 更新根 `Cargo.toml` — 工作区成员与共享依赖

**文件**：`d:\GitCode\RF\rust-agent-framework\Cargo.toml`

**变更内容**：

* 在 `[workspace.members]` 中添加 `"crates/wiki"`

* 在 `[workspace.dependencies]` 中添加 `llm-wiki` 所需的新第三方依赖：

```toml
# 全文搜索
tantivy = "0.22"          # 对齐项目现有生态（llm-wiki 使用 0.26，但考虑稳定性使用 0.22）
# YAML
serde_yaml = "0.9"
# JSON Schema 验证
jsonschema = "0.18"
# 图谱
petgraph = "0.6"
# 文件遍历
walkdir = "2"
# 密码学哈希
sha2 = "0.10"
hex = "0.4"
# 正则
regex = "1"
# 文件系统监控
notify = { version = "6", default-features = false, features = ["macos_kqueue"] }
# TOML 配置
toml = "0.8"
# 模板字符串
strum = { version = "0.26", features = ["derive"] }
strum_macros = "0.26"
```

### 2. 创建 `crates/wiki/Cargo.toml`

**文件**：`d:\GitCode\RF\rust-agent-framework\crates\wiki\Cargo.toml`（新建）

遵循现有 crate 模式：

```toml
[package]
name = "rust-agent-wiki"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "File-system based wiki engine with full-text search and concept graphs"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
regex = { workspace = true }

tantivy = { workspace = true }
petgraph = { workspace = true }
walkdir = { workspace = true }
jsonschema = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }
toml = { workspace = true }
notify = { workspace = true }
strum = { workspace = true }
strum_macros = { workspace = true }
```

### 3. 创建 `crates/wiki/src/` 源码目录

以下表格列出了所有需要创建的源文件，分别注明来源与修改说明。

#### 3.1 核心模块（直接照抄，仅修改 crate 内部引用）

| 文件                       | 来源                                          | 说明                                          |
| ------------------------ | ------------------------------------------- | ------------------------------------------- |
| `src/config.rs`          | `D:\Github\llm-wiki\src\config.rs`          | 直接照抄，无 hgit/mcp/acp 依赖                      |
| `src/frontmatter.rs`     | `D:\Github\llm-wiki\src\frontmatter.rs`     | 直接照抄                                        |
| `src/slug.rs`            | `D:\Github\llm-wiki\src\slug.rs`            | 直接照抄                                        |
| `src/markdown.rs`        | `D:\Github\llm-wiki\src\markdown.rs`        | 直接照抄                                        |
| `src/search.rs`          | `D:\Github\llm-wiki\src\search.rs`          | 直接照抄                                        |
| `src/links.rs`           | `D:\Github\llm-wiki\src\links.rs`           | 直接照抄                                        |
| `src/default_schemas.rs` | `D:\Github\llm-wiki\src\default_schemas.rs` | 直接照抄（含 `include_str!` 的 schema/template 文件） |
| `src/graph.rs`           | `D:\Github\llm-wiki\src\graph.rs`           | 直接照抄，不依赖 git/mcp/acp                        |
| `src/type_registry.rs`   | `D:\Github\llm-wiki\src\type_registry.rs`   | 直接照抄                                        |
| `src/space_builder.rs`   | `D:\Github\llm-wiki\src\space_builder.rs`   | 直接照抄                                        |
| `src/index_schema.rs`    | `D:\Github\llm-wiki\src\index_schema.rs`    | 直接照抄                                        |

#### 3.2 需要适配的核心模块

| 文件                     | 来源                                        | 修改说明                                                                                                                                         |
| ---------------------- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/engine.rs`        | `D:\Github\llm-wiki\src\engine.rs`        | 移除 `git::*` 导入与调用；`refresh_index()` / `staleness_check()` 改为基于文件 mtime 的实现；移除 `commit()` 方法；移除 ACP 相关字段引用                                    |
| `src/index_manager.rs` | `D:\Github\llm-wiki\src\index_manager.rs` | 移除 `git::changed_wiki_files()` / `git::changed_since_commit()` 调用；`update()` 方法改为基于文件修改时间或全量遍历文件后比较；`staleness_kind()` 移除 `CommitChanged` 变体 |
| `src/ingest.rs`        | `D:\Github\llm-wiki\src\ingest.rs`        | 移除 `auto_commit` 和 `changed_paths` 字段；移除 git commit 相关逻辑；保留文件遍历、验证、密文过滤核心逻辑                                                                  |
| `src/spaces.rs`        | `D:\Github\llm-wiki\src\spaces.rs`        | 移除 `git::init_repo()` 调用；`create()` 不再初始化 git 仓库；`register()` 改为仅验证目录结构                                                                      |
| `src/watch.rs`         | `D:\Github\llm-wiki\src\watch.rs`         | 移除 ACP 会话推送相关的所有逻辑（`notify` 仍在用，只移除 `acp` 引用）；保留 `notify` 文件监控、文件变更分类、索引重建调度                                                                 |

#### 3.3 `ops/` 模块（操作层）

| 文件                   | 来源                                  | 说明                       |
| -------------------- | ----------------------------------- | ------------------------ |
| `src/ops/mod.rs`     | `D:\Github\llm-wiki\src\ops\mod.rs` | 重新导出，排除 `history`、`logs` |
| `src/ops/config.rs`  | 照抄                                  | 无 git/mcp/acp 依赖         |
| `src/ops/content.rs` | 照抄，去除 `commit` 相关                   | 移除 `git::commit()` 调用    |
| `src/ops/export.rs`  | 照抄                                  | 无 git/mcp/acp 依赖         |
| `src/ops/graph.rs`   | 照抄                                  | 无 git/mcp/acp 依赖         |
| `src/ops/index.rs`   | 照抄，去除 commit 相关                     | 移除 git commit 引用         |
| `src/ops/ingest.rs`  | 照抄，去除 auto\_commit                  | 移除 git commit 相关逻辑       |
| `src/ops/lint.rs`    | 照抄                                  | 无 git/mcp/acp 依赖         |
| `src/ops/redact.rs`  | 照抄                                  | 纯文本处理，无依赖                |
| `src/ops/schema.rs`  | 照抄                                  | 无 git/mcp/acp 依赖         |
| `src/ops/search.rs`  | 照抄                                  | 无 git/mcp/acp 依赖         |
| `src/ops/spaces.rs`  | 照抄，去除 git init                      | 移除 `git::init_repo()` 调用 |
| `src/ops/stats.rs`   | 照抄                                  | 无 git/mcp/acp 依赖         |
| `src/ops/suggest.rs` | 照抄                                  | 无 git/mcp/acp 依赖         |

#### 3.4 排除的模块

| 模块                   | 原因                            |
| -------------------- | ----------------------------- |
| `src/git.rs`         | git 操作                        |
| `src/mcp/`（4 文件）     | MCP 传输                        |
| `src/acp/`（7 文件）     | ACP 传输                        |
| `src/server.rs`      | HTTP 服务器编排（依赖 mcp/acp）        |
| `src/cli.rs`         | CLI 定义（库不需要命令行）               |
| `src/main.rs`        | 二进制入口                         |
| `src/ops/history.rs` | 依赖 git 历史                     |
| `src/ops/logs.rs`    | 依赖 tracing-appender（库不需要日志管理） |

#### 3.5 新增资源文件

需要将 `D:\Github\llm-wiki\schemas\` 下的 JSON Schema 和 Markdown 模板复制到 `crates/wiki/schemas/`，因为 `default_schemas.rs` 使用 `include_str!()` 嵌入这些文件：

| 资源文件                        | 用途                |
| --------------------------- | ----------------- |
| `schemas/base.json`         | 基础页面 schema       |
| `schemas/concept.json`      | Concept 类型 schema |
| `schemas/paper.json`        | Paper 类型 schema   |
| `schemas/doc.json`          | Doc 类型 schema     |
| `schemas/section.json`      | Section 类型 schema |
| `schemas/skill.json`        | Skill 类型 schema   |
| `templates/concept.md`      | Concept 模板        |
| `templates/paper.md`        | Paper 模板          |
| `templates/doc.md`          | Doc 模板            |
| `templates/section.md`      | Section 模板        |
| `templates/query-result.md` | 查询结果模板            |

### 4. 创建 `crates/wiki/src/lib.rs`

```rust
// 公共模块
pub mod config;
pub mod default_schemas;
pub mod engine;
pub mod frontmatter;
pub mod graph;
pub mod index_manager;
pub mod index_schema;
pub mod ingest;
pub mod links;
pub mod markdown;
pub mod ops;
pub mod search;
pub mod slug;
pub mod space_builder;
pub mod spaces;
pub mod type_registry;
pub mod watch;

// 重导出核心类型
pub use config::{GlobalConfig, WikiConfig, ResolvedConfig};
pub use engine::{WikiEngine, EngineState, SpaceContext};
pub use slug::{Slug, WikiUri, ReadTarget};
// ... 按需添加
```

***

## 关键适配决策

### 引擎层面的 Git 替代方案

`llm-wiki` 的 `WikiEngine` 通过 git commit hash 来判断索引是否过期。移除 git 后：

1. **`SpaceIndexManager::staleness_kind()`**：

   * 移除 `CommitChanged` 变体

   * 保留 `Current`、`TypesChanged`、`FullRebuildNeeded`

   * 通过比较 schema hash（已有机制）和文件 mtime 来判断

2. **`SpaceIndexManager::update()`**：

   * 移除 `git::changed_since_commit()` 调用

   * 改为遍历 wiki 目录，对比文件 mtime 与索引中记录的文档时间戳

3. **`engine::refresh_index()`**：简化为 `update()` 的薄包装

4. **`spaces::create()`**：不再初始化 git 仓库

5. **`ingest.rs`**：移除 `auto_commit` 选项和 `changed_paths` 字段

### 依赖版本选择

* `tantivy = "0.22"` — 使用与项目现有生态兼容的版本（需确认 `rust-agent-rag` 等是否已使用）

* 其他依赖使用相对较新且稳定的版本

***

## 验证步骤

1. **编译检查**：`cargo build -p rust-agent-wiki`
2. **单元测试**：`cargo test -p rust-agent-wiki`
3. **类型检查**：`cargo check -p rust-agent-wiki`
4. **集成验证**：编写一个简单示例，验证以下流程：

   * 创建 wiki 空间

   * 写入 Markdown 页面（含 frontmatter）

   * 构建全文搜索索引

   * 执行搜索查询

   * 构建概念图谱

