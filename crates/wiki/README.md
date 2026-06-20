# rust-agent-wiki

基于文件系统的 Wiki 引擎，提供全文搜索、类型化页面和概念图谱 —— Rust Agent Framework 的基础设施组件。

设计思想源自 Andrej Karpathy 的 [LLM Wiki 模式](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)，代码基于 [geronimo-iia/llm-wiki](https://github.com/geronimo-iia/llm-wiki) 适配而来，移除了 git、MCP、ACP 传输层，定位为可嵌入 Agent 运行时的库 crate。

## 架构

遵循 LLM Wiki 的三层架构：

```
原始素材 (inbox/, raw/) ──► Wiki 页面 (wiki/*.md) ──► Schema (schemas/*.json)
  不可变，LLM 只读         LLM 编写和维护             JSON Schema + 模板
```

引擎提供以下能力：

- **类型化页面** — 基于 JSON Schema 校验的 YAML frontmatter，支持多种注册页面类型（concept、paper、doc、section、skill 等）
- **全文搜索** — Tantivy BM25 索引，支持分面统计和跨 Wiki 查询
- **概念图谱** — 带类型标注的有向图，支持 Louvain 社区检测，可渲染为 Mermaid/DOT 格式
- **Wiki 链接** — 支持 `[[页面slug]]` 和 `wiki://名称/slug` 两种链接格式的提取与校验
- **Lint 规则** — 孤立页面检测、死链检测、缺失字段、过期页面、未知类型等规则
- **推荐引擎** — 四种策略的关联页面推荐（标签重叠、图谱邻域、BM25 相似度、社区同伴）
- **密文过滤** — 内置常用 API Key / Token / 凭证的正则过滤规则
- **文件监控** — 基于 `notify` 的文件变更监听，自动触发索引热重载

## v2 仿生记忆特性

基于 Karpathy LLM Wiki v2 规范，引擎实现了 8 项仿生记忆能力，使知识库具备生命周期管理：

| 特性 | 模块 | 说明 |
|------|------|------|
| **置信度评分** | `confidence` | 动态权重：`base × source_reliability × evidence_factor × freshness` |
| **分层记忆** | `memory` | 四层架构：working（工作）/ episodic（情节）/ semantic（语义）/ procedural（程序） |
| **知识图谱** | `graph` | 带类型实体 + 带类型关系 + 图遍历（已存在于 v1，v2 复用） |
| **混合检索** | `hybrid` | BM25 + 向量 + 图遍历，RRF 融合（`score = Σ 1/(60 + rank_i)`） |
| **遗忘曲线** | `forgetting` | Ebbinghaus 衰减：`retention = exp(-age/halflife)`，按类型配置半衰期 |
| **自动治理** | `governance` | 自动摄取、自动压缩、周期性清理冗余页面 |
| **冲突解决** | `conflict` | 检测矛盾声明，按权威性 + 时效性提议解决方案 |
| **写入门控** | `gate` | 选择性记忆：未过滤存储会使准确率从 100% 降至 13% |

### 启用 v2 特性

```rust
use rust_agent_wiki::WikiEngine;

let engine = WikiEngine::from_repo("my-wiki", "/path/to/repo")?;

// 启用向量检索（混合搜索前置条件）
engine.enable_vector_search("my-wiki")?;
engine.build_vector_index("my-wiki").await?; // 返回索引的 chunk 数

// 启用分层记忆
engine.enable_memory("my-wiki")?;
```

### 混合检索

```rust
use rust_agent_wiki::ops::{search, SearchParams};

let result = search(
    &engine.state.read().unwrap(),
    "my-wiki",
    &SearchParams {
        query: "所有权机制",
        top_k: Some(10),
        hybrid: true,              // 启用 BM25 + 向量 + 图遍历 RRF 融合
        vector_weight: Some(1.0),  // 可选：向量权重
        graph_weight: Some(0.5),   // 可选：图遍历权重
        graph_hops: Some(1),       // 可选：图遍历跳数
        ..Default::default()
    },
)?;
```

### 写入门控

```rust
use rust_agent_wiki::ops::content::content_write_gated;

// 评估门控规则后再写入：拒绝低置信度、空内容、重复 slug
let result = content_write_gated(
    &state,
    "concepts/rust-ownership",
    None,
    "---\ntitle: Rust 所有权\ntype: concept\nconfidence: 0.9\n---\n\n...",
)?;
match result.decision {
    crate::gate::GateDecision::Accept => println!("已写入"),
    crate::gate::GateDecision::Reject(reason) => println!("拒绝: {reason}"),
    crate::gate::GateDecision::NeedsReview(reason) => println!("需审查: {reason}"),
}
```

### v2 配置

在 `wiki.toml` 或全局 `config.toml` 中配置 v2 段：

```toml
[decay]
default_halflife_days = 90
forget_threshold = 0.2
archive_threshold = 0.05
[decay.halflife_by_type]
concept = 365
spec = 365
bug = 30

[gate]
min_confidence = 0.2
reject_confidence = 0.05
min_body_length = 10
duplicate_threshold = 0.9

[governance]
interval_secs = 3600
redundancy_confidence_threshold = 0.1
redundancy_age_days = 180

[memory]
working_capacity = 100
compress_threshold = 20
promote_confidence = 0.7
```

`stale` lint 规则已集成遗忘曲线：`Archivable` 状态页面报 `error`，`Forgettable` 状态报 `warning`，并在消息中显示保留率与衰减后置信度。

## 快速开始

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
rust-agent-wiki = { path = "crates/wiki" }
```

### 初始化和使用

```rust
use std::path::Path;
use rust_agent_wiki::{WikiEngine, ops};

fn main() -> anyhow::Result<()> {
    // 1. 创建全局配置目录
    let config_dir = dirs::home_dir().unwrap().join(".my-wiki");
    std::fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("config.toml");

    // 2. 创建一个 Wiki 空间（包括目录结构、默认 Schema 和模板）
    ops::spaces::spaces_create(
        Path::new("/path/to/my-wiki-repo"),
        "my-wiki",
        Some("我的知识库"),
        false,  // force — 是否强制覆盖已有注册
        true,   // set_default — 设为默认 Wiki
        &config_path,
        None,   // wiki_root — 页面目录，默认 "wiki"
    )?;

    // 3. 构建引擎（挂载所有已注册 Wiki，构建搜索索引）
    let engine = WikiEngine::build(&config_path)?;

    Ok(())
}
```

### 创建页面

```rust
use rust_agent_wiki::ops;

let state = engine.state.read().unwrap();

let result = ops::content::content_new(
    &state,
    "concepts/rust-lifetimes",  // slug — 页面标识符
    None,   // wiki_flag — 使用默认 Wiki
    false,  // is_section — 是否为目录页
    false,  // bundle — 是否创建为 bundle（目录 + index.md）
    Some("Rust 生命周期"),
    Some("concept"),
)?;

// 将创建文件：wiki/concepts/rust-lifetimes.md
// 包含自动生成的 YAML frontmatter：title, type, status, tags 等
```

### 写入页面内容

```rust
let state = engine.state.read().unwrap();

ops::content::content_write(
    &state,
    "concepts/rust-lifetimes",
    None,
    "## 概述\n\n生命周期是 Rust 用来确保引用始终有效的机制...\n",
)?;
```

### 读取页面

```rust
let state = engine.state.read().unwrap();

match ops::content::content_read(&state, "concepts/rust-lifetimes", None, false, false)? {
    ops::content::ContentReadResult::Page(content) => {
        println!("{}", content);
    }
    _ => {}
}
```

### 搜索

```rust
let result = ops::search::search(
    &engine,
    "my-wiki",
    &search::SearchParams {
        query: "所有权".to_string(),
        top_k: 10,
        ..Default::default()
    },
)?;

for page in &result.results {
    println!("{} (score: {:.2}): {}", page.title, page.score, page.summary);
}
```

### 构建和渲染概念图谱

```rust
let result = ops::graph::graph_build(
    &engine,
    "my-wiki",
    &graph::GraphParams {
        format: "mermaid".to_string(),
        depth: Some(3),
        ..Default::default()
    },
)?;

// result.output 包含 Mermaid 流程图代码
```

### 执行 Lint 检查

```rust
let report = ops::lint::run_lint(
    &engine.state.read().unwrap(),
    "my-wiki",
    None,  // 运行所有规则
    None,  // 输出所有严重级别
)?;

println!("{} 个错误, {} 个警告", report.errors, report.warnings);
```

### 获取推荐（关联页面）

```rust
let suggestions = ops::suggest::suggest(
    &engine.state.read().unwrap(),
    "concepts/rust-lifetimes",
    None,      // wiki_flag
    Some(5),   // limit — 返回上限
)?;

for s in &suggestions {
    println!("[{}] {} — {}", s.field, s.title, s.reason);
}
```

### Wiki 统计

```rust
let stats = ops::stats::stats(&engine.state.read().unwrap(), "my-wiki")?;
println!("{} 个页面, {} 个目录, {} 个孤立页面",
    stats.pages, stats.sections, stats.orphans);
```

### 摄入（验证文件）

```rust
let state = engine.state.read().unwrap();
let space = state.space("my-wiki")?;

let report = ops::ingest::ingest(
    &state,
    &engine,
    "concepts/",
    false,   // dry_run — 仅验证不写入
    "my-wiki",
)?;

println!("{} 个页面通过验证, {} 个警告", report.pages_validated, report.warnings.len());
```

### 导出

```rust
let report = ops::export::export(
    &engine,
    &ops::export::ExportOptions {
        wiki: Some("my-wiki".to_string()),
        path: Some("export.txt".to_string()),
        format: "llms-txt".to_string(),
        include_archived: false,
    },
)?;
```

## 与 Rust Agent Framework 集成

### 作为 Agent 工具注册

在 `chat_client_decorators/function_invoking.rs` 中将 Wiki 操作注册为 Agent 可调用的工具：

```rust
use rust_agent_wiki::WikiEngine;

// 在多个 Agent 会话间共享同一个引擎实例
struct WikiSearchTool {
    engine: WikiEngine,
}

#[async_trait::async_trait]
impl ToolTrait for WikiSearchTool {
    fn name(&self) -> &str { "wiki_search" }
    fn description(&self) -> &str {
        "在 Wiki 知识库中搜索相关页面"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("");
        let wiki = args["wiki"].as_str();

        let params = search::SearchParams {
            query: query.to_string(),
            ..Default::default()
        };
        let result = ops::search::search(
            &self.engine,
            wiki.unwrap_or("default"),
            &params,
        )?;

        // 渲染为 LLM 友好的带引文文本
        let text = search::render_search_llms(&result);
        Ok(ToolResult::text(text))
    }
}

struct WikiWriteTool {
    engine: WikiEngine,
}

#[async_trait::async_trait]
impl ToolTrait for WikiWriteTool {
    fn name(&self) -> &str { "wiki_write" }
    fn description(&self) -> &str {
        "写入或更新 Wiki 页面。LLM 是页面的主要作者。"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let slug = args["slug"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let state = self.engine.state.read().unwrap();
        let result = ops::content::content_write(&state, slug, None, content)?;
        Ok(ToolResult::text(format!("页面已写入: {}", result.path.display())))
    }
}
```

### 作为上下文提供器

在 `context_providers/` 中注入基于 Wiki 的上下文，让 Agent 对话获得知识库支撑：

```rust
impl ContextProvider for WikiContextProvider {
    async fn provide(&self, messages: &[Message]) -> String {
        // 从最近的对话消息中提取主题，搜索 Wiki
        let query = extract_query(messages);
        let result = ops::search::search(&self.engine, "default", &params)?;

        // 格式化为上下文块
        let mut ctx = String::from("## Wiki 知识库\n\n");
        for page in &result.results {
            ctx.push_str(&format!(
                "- [{}](wiki://default/{})\n", page.title, page.slug
            ));
        }
        ctx
    }
}
```

### Websearch → Wiki 管道

`websearch` / `websearch-ai` crate 的搜索结果可以自动摄入为 Wiki 页面：

```rust
// 在 websearch 返回结果之后
for result in web_results {
    let state = engine.state.read().unwrap();
    // 创建页面
    ops::content::content_new(
        &state,
        &format!("sources/{}", slugify(&result.title)),
        None, false, false,
        Some(&result.title),
        Some("article"),
    )?;

    // 写入内容
    ops::content::content_write(
        &state,
        &format!("sources/{}", slugify(&result.title)),
        None,
        &format!("# {}\n\n{}\n\n来源: {}", result.title, result.snippet, result.url),
    )?;
}

// 批量摄入后刷新索引
engine.rebuild_index("default")?;
```

## Wiki 目录结构

每个 Wiki 仓库的文件布局如下：

```
wiki-repo/
├── wiki.toml          # 单 Wiki 配置（类型覆盖、wiki 根路径等）
├── README.md
├── inbox/             # 待摄入文件的暂存区
├── raw/               # 不可变的原始文档
├── schemas/           # JSON Schema 文件 + Markdown 正文模板
│   ├── base.json
│   ├── concept.json
│   ├── paper.json
│   ├── doc.json
│   ├── section.json
│   ├── skill.json
│   ├── concept.md
│   ├── paper.md
│   └── ...
└── wiki/              # LLM 维护的 Markdown 页面（可通过 wiki_root 自定义路径）
    ├── index.md
    ├── concepts/
    │   └── rust-ownership.md
    └── sources/
        └── karpathy-llm-wiki.md
```

## 页面格式

Wiki 页面使用 YAML frontmatter，字段由 JSON Schema 校验：

```markdown
---
title: Rust 所有权
type: concept
status: active
tags: [rust, memory, ownership]
summary: Rust 内存安全模型的核心概念
confidence: 0.9
last_updated: 2026-06-16
sources:
  - sources/rust-book-ch4
concepts:
  - concepts/rust-borrowing
  - concepts/rust-lifetimes
---

## 概述

所有权是 Rust 最独特的特性...
```

## Schema 系统

页面类型由 `schemas/` 目录下的 JSON Schema 文件定义。每个 Schema 可以声明：

- **`x-wiki-types`** — 类型名 → 描述的映射，用于类型发现
- **`x-graph-edges`** — 带类型标注的图边声明，包含方向和目标类型约束
- **`x-index-aliases`** — 搜索索引的字段别名

图边声明示例：

```json
{
  "x-graph-edges": [
    {
      "field": "sources",
      "relation": "fed-by",
      "direction": "outgoing",
      "target_types": ["paper", "article", "clipping"]
    }
  ]
}
```

## 配置系统

三层配置体系：

1. **全局配置** (`~/.my-wiki/config.toml`) — 默认 Wiki、索引设置、图谱设置、所有已注册的 Wiki
2. **单 Wiki 配置** (`<repo>/wiki.toml`) — 覆盖类型注册表、默认参数、读取设置、图谱设置
3. **运行时合并** — 通过 `config::resolve(global, per_wiki)` 动态合并

## API 参考

### `WikiEngine`（从 `rust_agent_wiki` 重导出）

| 方法 | 说明 |
|------|------|
| `build(config_path)` | 加载全局配置，挂载所有已注册的 Wiki |
| `rebuild_index(wiki_name)` | 全量重建 Tantivy 搜索索引 |
| `schema_rebuild(wiki_name)` | 智能重建（仅类型变更时做部分重建） |
| `mount_wiki(entry)` | 热挂载一个 Wiki 到运行中的引擎 |
| `unmount_wiki(name)` | 热卸载一个 Wiki |
| `set_default(name)` | 更新默认 Wiki |

### `ops` 模块

| 模块 | 核心函数 |
|------|---------|
| `ops::spaces` | `spaces_create`、`spaces_register`、`spaces_list`、`spaces_remove`、`spaces_set_default` |
| `ops::content` | `content_read`、`content_write`、`content_new`、`content_write_gated`（v2 门控写入）、`backlinks_for` |
| `ops::search` | `search`（支持 `hybrid=true` 混合检索）、`list` — BM25/混合搜索和分面统计 |
| `ops::graph` | `graph_build` — Mermaid / DOT / LLM 文本三种渲染格式 |
| `ops::lint` | `run_lint` — 6 条规则（`stale` 已集成 v2 遗忘曲线） |
| `ops::suggest` | `suggest` — 4 策略关联页面推荐 |
| `ops::stats` | `stats` — 页面统计、图谱指标、社区统计 |
| `ops::ingest` | `ingest`、`ingest_with_redact` — 文件验证，可选密文过滤 |
| `ops::export` | `export` — llms.txt、llms-full、JSON 三种导出格式 |
| `ops::schema` | `schema_list`、`schema_show`、`schema_add`、`schema_remove`、`schema_validate` |
| `ops::index` | `index_rebuild`、`index_status` |
| `ops::config` | `config_get`、`config_set`、`config_list_global`、`config_list_resolved` |
| `ops::redact` | `redact_body` — 基于正则的敏感信息过滤 |

### v2 模块

| 模块 | 核心类型/函数 |
|------|--------------|
| `confidence` | `compute`、`ConfidenceInput`、`ConfidenceBreakdown`、`default_source_reliability` |
| `forgetting` | `decay`、`decay_from_frontmatter`、`reinforce`、`DecayConfig`、`DecayStatus` |
| `memory` | `MemoryStore`、`MemoryTier`、`MemoryConfig`（`observe`/`compress`/`promote`/`sediment_procedural`） |
| `hybrid` | `hybrid_search`、`HybridParams`、`render_hybrid_llms`（RRF 融合） |
| `gate` | `evaluate`、`GateConfig`、`GateDecision`（Accept/Reject/NeedsReview） |
| `conflict` | `detect`、`propose_resolution`、`authority_score`、`Conflict`、`Resolution` |
| `governance` | `GovernanceScheduler`、`GovernanceConfig`、`GovernanceTask` |
| `vector` | `VectorIndex`（`index_page`/`search`/`remove_page`/`index_page_file`） |

## 与上游 llm-wiki 的差异

| 特性 | llm-wiki | rust-agent-wiki |
|------|----------|-----------------|
| Git 集成 | 基于 `git2` 的提交、diff、历史 | 已移除 |
| MCP 服务端 | `rmcp` stdio + Streamable HTTP | 已移除 |
| ACP 服务端 | `agent-client-protocol` 工作流 | 已移除 |
| CLI 二进制 | 基于 `clap` 的 17 个子命令 | 已移除（仅库） |
| 图谱快照 | `petgraph-live` 快照缓存 | 仅内存缓存 |
| 结构分析算法 | 直径、半径、中心点、边缘节点 | 暂不支持（petgraph-live 不可用） |
| 索引过期判断 | 基于 Git commit hash 对比 | 基于 Schema hash + 文件存在性 |
| 依赖管理 | 独立 Cargo.toml | 工作区级 `[workspace.dependencies]` |
| 包名 | `llm-wiki-engine` | `rust-agent-wiki` |

## 依赖项

| Crate | 用途 |
|-------|------|
| `tantivy 0.22` | BM25 全文搜索 |
| `petgraph 0.6` | 概念有向图 + Louvain 社区检测 |
| `rust-agent-rag` | v2 向量检索（`IVectorStore`、`IEmbeddingModel`、`SimilarityRetriever`） |
| `parking_lot` | v2 `SpaceContext` 字段内部可变性 |
| `jsonschema 0.26` | JSON Schema frontmatter 校验 |
| `serde_yaml` | YAML frontmatter 解析 |
| `walkdir` | 索引遍历 |
| `notify` | 文件系统监控 |
| `chrono` | 过期检测时间计算 |
| `sha2` + `hex` | Schema 内容哈希 |
| `regex` | Wiki 链接提取和密文过滤 |
