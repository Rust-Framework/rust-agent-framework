# rust-agent-llama

本地 GGUF 模型推理，基于 [llama-gguf](https://crates.io/crates/llama-gguf)，实现 RAF 的 `IChatClient` 接口。

## 依赖

```toml
rust-agent-llama = { path = "../llama" }
```

## 快速开始

```rust
use rust_agent_llama::{LlamaChatClient, LlamaChatClientOptions};

let client = LlamaChatClient::new(LlamaChatClientOptions::new(
    "path/to/model.gguf",
    "llama-3.2-1b-it",
))?;
```

GGUF 文件内嵌 tokenizer，通常无需单独指定 `tokenizer_path`。

## 声明式配置

在 `rust-agent-decl` 中启用 `llama` feature，使用 `provider: llama`（或 `gguf` / `lm` / `local`）：

```toml
[dependencies]
rust-agent-decl = { path = "../decl", features = ["llama"] }
```

```yaml
provider: llama
connection:
  endpoint: /path/to/model.gguf
```

## 构建

```bash
cargo build --release -p rust-agent-llama
```

可选 GPU：在 `LlamaChatClientOptions` 中设置 `use_gpu: true`（需 llama-gguf 对应后端 feature）。
