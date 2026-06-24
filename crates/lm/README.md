# rust-agent-lm

基于 [lm.rs](https://github.com/samuel-vitorino/lm.rs) 的**本地 CPU 推理**模块，为 RAF 提供 [`IChatClient`](https://github.com/rf2026/rust-agent-framework) 实现。无需 API Key、无需网络，在纯 CPU 上运行 Gemma 2、Llama 3.2、Phi-3.5 等量化模型。

> 详细文档见：[9.5 本地模型推理](../../docs/09-chat-client-pipeline/local-inference.md)

## 适用场景

| 场景 | 说明 |
|------|------|
| 离线开发 | 无网络环境下调试 Agent 流程 |
| 隐私敏感 | 数据不出本机 |
| 成本可控 | 无按 token 计费 |
| 原型验证 | 快速验证 prompt / 工具编排逻辑 |

不适合：需要强工具调用能力、长上下文（>8K）、多模态 vision、高吞吐生产部署。

## 快速开始

### 1. 准备模型文件

从 [Hugging Face lmrs 合集](https://huggingface.co/collections/samuel-vitorino/lmrs-66c7da8a50ce52b61bee70b7) 下载 **LMRS 格式**的权重与 tokenizer，例如：

```
models/
├── llama-3.2-1b-it-q8_0.lmrs    # 模型权重（~1.3 GB）
└── tokenizer.bin                 # 配套 tokenizer
```

推荐入门模型：**Llama 3.2 1B IT Q8_0**（体积小、速度快，16 核 CPU 约 50 tok/s）。

### 2. 添加依赖

```toml
[dependencies]
rust-agent-core = { path = "../core" }
rust-agent-lm = { path = "../lm" }
tokio = { version = "1", features = ["full"] }
futures-util = "0.3"
```

### 3. 代码调用

```rust
use futures_util::StreamExt;
use rust_agent_core::{
    collect_agent_response, ChatClientRunOptions, ChatMessage, IChatClient,
};
use rust_agent_lm::{LmChatClient, LmChatClientOptions};

#[tokio::main]
async fn main() -> rust_agent_core::Result<()> {
    let client = LmChatClient::new(LmChatClientOptions::new(
        "models/llama-3.2-1b-it-q8_0.lmrs",
        "models/tokenizer.bin",
        "llama-3.2-1b-it",
    ))?;

    let messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::user("用一句话介绍 Rust。"),
    ];

    let stream = client
        .run(&messages, ChatClientRunOptions::default())
        .await?;

    let response = collect_agent_response(stream).await?;
    println!("{}", response.text);
    Ok(())
}
```

### 4. 接入 Agent 管道

`LmChatClient` 实现标准 `IChatClient`，可作为 `ChatClientBuilder` 的叶子节点：

```rust
use std::sync::Arc;
use rust_agent_core::ChatClientBuilder;
use rust_agent_lm::{LmChatClient, LmChatClientOptions};

let leaf = Arc::new(LmChatClient::new(options)?);
let pipeline = ChatClientBuilder::new()
    .leaf(leaf)
    // .use_decorator(...)  // 可叠加 FunctionInvoking 等装饰器
    .build()?;
```

## 配置项

[`LmChatClientOptions`](src/options.rs)：

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `model_path` | `String` | — | LMRS 权重文件路径（必填） |
| `tokenizer_path` | `String` | — | LMRS tokenizer 路径（必填） |
| `model_id` | `String` | — | 逻辑模型名，用于日志与元数据 |
| `temperature` | `Option<f32>` | `0.7` | 采样温度；`0.0` = greedy |
| `top_p` | `Option<f32>` | `0.9` | Nucleus sampling |
| `max_tokens` | `Option<u32>` | `512` | 单次最大生成 token 数 |
| `seed` | `Option<u64>` | 随机 | 固定种子可复现输出 |
| `model_metadata` | `Option<ModelMetadata>` | 自动推断 | 上下文窗口等能力边界 |

单次调用可通过 `ChatClientRunOptions` 覆盖 `temperature`、`top_p`、`max_tokens`。

## 声明式配置

在 `rust-agent-decl` 中启用 `lm` feature：

```toml
[dependencies]
rust-agent-decl = { path = "../decl", features = ["lm"] }
```

```yaml
# agent.yaml
kind: prompt
name: local-assistant
instructions: You are a helpful assistant.
model:
  id: llama-3.2-1b-it
  provider: lm          # 或 local
  connection:
    kind: anonymous
    endpoint: /path/to/model.lmrs
  options:
    kind: chat
    temperature: 0.7
    max_output_tokens: 256
    top_p: 0.9
    tokenizer_path: /path/to/tokenizer.bin
```

| 配置项 | 说明 |
|--------|------|
| `connection.endpoint` | 模型权重 `.lmrs` 文件路径 |
| `options.tokenizer_path` | tokenizer 路径；省略时默认 `tokenizer.bin` |
| `connection` 无需 `api_key` | 本地推理不需要密钥 |

## 模型文件准备

### 方式 A：直接下载（推荐）

1. 打开 [lmrs Hugging Face 合集](https://huggingface.co/collections/samuel-vitorino/lmrs-66c7da8a50ce52b61bee70b7)
2. 选择模型（见下表），下载 `.lmrs` 权重和 `tokenizer.bin`
3. 放到本地目录，在配置中填写绝对或相对路径

### 方式 B：自行转换

若需从官方 Hugging Face 权重转换，使用 lm.rs 仓库中的 Python 脚本：

```bash
git clone https://github.com/samuel-vitorino/lm.rs
cd lm.rs
pip install -r requirements.txt

# 1. 转换模型权重
python export.py \
  --files model-00001-of-00001.safetensors \
  --config config.json \
  --save-path ./out/model.lmrs \
  --type LLAMA \
  --quantize --quantize-type Q8_0

# 2. 转换 tokenizer
python tokenizer.py \
  --model-id meta-llama/Llama-3.2-1B-Instruct \
  --tokenizer-type LLAMA
```

`--type` / `--tokenizer-type` 取值：`GEMMA`、`LLAMA`、`PHI`。

### 推荐模型与性能参考

在 16 核 AMD Epyc 上的参考速度（来自 lm.rs 官方 benchmark）：

| 模型 | 文件大小 | 速度 |
|------|----------|------|
| Llama 3.2 1B IT Q8_0 | ~1.27 GB | ~50 tok/s |
| Llama 3.2 3B IT Q8_0 | ~3.31 GB | ~19 tok/s |
| Gemma 2 2B IT Q8_0 | ~2.66 GB | ~24 tok/s |
| Gemma 2 9B IT Q8_0 | ~9.53 GB | ~8 tok/s |
| Phi 3.5 Mini IT Q8_0 | ~3.94 GB | ~18 tok/s |

量化建议：优先 **Q8_0**（质量与体积平衡）；Q4_0 体积更小但质量仍在改进中。

### 文件格式说明

- **`.lmrs`**：lm.rs 专有二进制格式，文件头魔数为 `lmrs`（`0x6c6d7273`）
- **`tokenizer.bin`**：lm.rs 专有 tokenizer 格式（vocab + BPE merge scores）
- 权重与 tokenizer **必须配对**（同一原始模型导出）

## 架构

```
ChatMessage[]
      │
      ▼
 prompt.rs          # 按 Gemma/Llama/Phi chat template 转 token
      │
      ▼
 LmEngine::generate  # CPU 推理（spawn_blocking）
      │
      ├── Transformer::forward (prefill + decode)
      ├── Sampler (temperature / top_p)
      └── Tokenizer::decode
      │
      ▼
 AgentResponseUpdate  # TextDelta → Usage → Finish
```

- 推理在 `tokio::task::spawn_blocking` 中执行，不阻塞 async runtime
- 每次 `run()` 从 mmap 重建 `Transformer`，完整重放历史消息
- 有效上下文上限 **8192 token**（lm.rs 内部上限）

## 当前限制

| 限制 | 说明 |
|------|------|
| 无工具调用 | `ChatClientRunOptions.tools` 被忽略（会打 warn 日志） |
| 无多模态 | 不支持 Phi-3.5 Vision 图像输入 |
| 无 KV cache 复用 | 每轮 `run()` 全量 prefill |
| CPU only | 无 GPU/CUDA 加速 |
| 启动日志 | `Transformer::new` 会向 stdout 打印模型信息（lm.rs 行为） |

## 性能调优

```bash
# 针对本机 CPU 指令集优化（推荐 release 构建）
RUSTFLAGS="-C target-cpu=native" cargo build --release -p rust-agent-lm
```

- 更多 CPU 核心可提升 matmul 并行度（lm.rs 使用 rayon）
- 减小 `max_tokens` 可缩短单次响应时间
- 控制 system prompt 与历史长度，避免触及 8192 token 上限

## 模块结构

| 文件 | 职责 |
|------|------|
| [`chat_client.rs`](src/chat_client.rs) | `LmChatClient` — `IChatClient` 实现 |
| [`engine.rs`](src/engine.rs) | `LmEngine` — mmap 加载、token 生成循环 |
| [`prompt.rs`](src/prompt.rs) | `ChatMessage` → token 序列转换 |
| [`options.rs`](src/options.rs) | `LmChatClientOptions` 配置 |

## 依赖

- [`lmrs`](https://github.com/samuel-vitorino/lm.rs) — 纯 Rust CPU 推理引擎
- `rust-agent-core` — `IChatClient` trait 与消息类型
- `memmap2` — 模型权重内存映射
- `parking_lot` — tokenizer 互斥锁

## 相关文档

- [9.5 本地模型推理（详细指南）](../../docs/09-chat-client-pipeline/local-inference.md)
- [9.3 LLM 提供商](../../docs/09-chat-client-pipeline/llm-providers.md)
- [10.5 声明式配置字段参考](../../docs/10-macros-declarative/config-reference.md)
