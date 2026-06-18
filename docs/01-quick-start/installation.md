# 环境安装

本文档介绍如何搭建 Rust Agent Framework (RAF) 开发环境，包括 Rust 工具链安装、项目创建和依赖配置。

## 前置条件

- **Rust 工具链**：`rustc >= 1.75.0`，推荐使用 `rustup` 安装：

```bash
rustup default stable
rustup update
```

- **Cargo 工作区**：RAF 采用 workspace 多 crate 架构，你的项目需要引用其中的 crate。

## 创建项目

```bash
cargo new my-raf-agent
cd my-raf-agent
```

## 配置 Cargo.toml

RAF 包含 15 个 crate，按依赖关系分为核心层、运行时层和扩展层。你需要按需引用。

### 最小依赖（纯 API 调用）

如果你只需要调用 LLM API，只需 `rust-agent-client` 和 `rust-agent-core`：

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
rust-agent-core = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-core" }
rust-agent-client = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-client" }
```

### 标准配置（构建智能体）

如果你需要完整的智能体运行时、工具注册、上下文管理和会话支持：

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
futures-util = "0.3"
serde_json = "1"

# 核心抽象层
rust-agent-core = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-core" }
# LLM 客户端（DeepSeek / OpenAI 兼容）
rust-agent-client = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-client" }
# 智能体框架运行时（AgentBuilder、ChatClientAgent、内置工具、压缩策略）
rust-agent-framework = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-framework" }
# 工具宏支持
rust-agent-macros = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-macros" }
```

### 本地开发（直接引用 workspace）

如果你克隆了 RAF 源码，可在 workspace 的 `Cargo.toml` 中按相对路径引用：

```toml
[dependencies]
rust-agent-core = { path = "../rust-agent-framework/crates/core" }
rust-agent-client = { path = "../rust-agent-framework/crates/client" }
rust-agent-framework = { path = "../rust-agent-framework/crates/framework" }
```

### 扩展依赖（按需添加）

```toml
# Web 搜索支持
rust-agent-websearch = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-websearch" }

# RAG（检索增强生成）
rust-agent-rag = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-rag" }

# Wiki 知识检索
rust-agent-wiki = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-wiki" }

# Rhai 脚本引擎
rust-agent-rhai = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-rhai" }

# 工作流引擎
rust-agent-workflow = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-workflow" }

# 声明式 Agent DSL
rust-agent-decl = { git = "https://gitcode.com/rf2026/rust-agent-framework.git", package = "rust-agent-decl" }
```

## 可选功能（Feature Flags）

`rust-agent-framework` 提供了 `tiktoken` feature，启用后可使用精确的 Token 计数器：

```toml
rust-agent-framework = { git = "...", package = "rust-agent-framework", features = ["tiktoken"] }
```

| Feature  | 说明 |
|----------|------|
| `tiktoken` | 使用 `tiktoken-rs` 实现精确的 Token 计数（`TiktokenCounter`），不启用时回退到 `EstimateCounter`（每 Token ≈ 4 个字符） |

## 验证安装

创建 `src/main.rs` 验证依赖是否正常解析：

```rust
use rust_agent_core::{ChatMessage, MessageRole};

fn main() {
    let msg = ChatMessage::user("Hello, RAF!");
    println!("{:?}", msg);
}
```

```bash
cargo run
```

若输出 `ChatMessage { role: User, content: "Hello, RAF!", ... }` 则安装成功。

## 所需 API Key

RAF 的 LLM 客户端需要 API Key。支持的提供商：

| 提供商 | 配置方式 |
|--------|----------|
| DeepSeek | `DeepSeekChatClient::from_key("sk-...", "deepseek-chat")` |
| OpenAI | `OpenAiChatClient::from_key(api_base, "sk-...", "gpt-4o")` |

也可通过环境变量传入：

```bash
export DEEPSEEK_API_KEY="sk-xxxxxxxx"
export OPENAI_API_KEY="sk-xxxxxxxx"
```

## 下一步

环境就绪后，继续阅读 **[第一个智能体](./first-agent.md)**，开始构建你的第一个 RAF Agent。
