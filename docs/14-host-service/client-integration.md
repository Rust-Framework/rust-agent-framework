# 14.8 客户端集成指南

本指南详细介绍如何将 IDE 客户端（或任何 ACP 兼容客户端）连接到 `rust-agent-host` 服务端，包括传输层选择、会话管理、每轮模型配置传递和流式输出处理。

## 快速开始

### 1. 启动 Host 服务端

**Stdio 模式**（本地子进程，标准 ACP 模式）：

```bash
cargo run -p rust-agent-host -- --mode stdio
```

**WebSocket 模式**（远程部署）：

```bash
cargo run -p rust-agent-host -- --mode ws --bind 127.0.0.1:9876
```

### 2. 配置 LLM 提供商

通过 `host.toml` 配置文件：

```toml
[provider]
provider = "openai"
model = "agnes-2.0-flash"
api_key = "$AGNES_API_KEY"
base_url = "https://apihub.agnes-ai.com/v1"
temperature = 0.3
max_tokens = 8192
context_window_tokens = 128000
max_output_tokens = 8192

[agents]
coding = true
general = true
analysis = true

[dev_pipeline]
enabled = true
agent_id = "dev-pipeline"
max_iterations = 3
```

或通过环境变量 / CLI 参数：

```bash
AGNES_API_KEY=sk-xxx \
cargo run -p rust-agent-host -- \
  --mode stdio \
  --provider deepseek \
  --model agnes-2.0-flash \
  --workspace-root /path/to/project
```

## 传输层集成

### Stdio 模式集成

适用于 IDE 将 host 作为本地子进程启动的场景。

```typescript
// TypeScript 示例：启动 host 子进程
import { spawn } from 'child_process';

const hostProcess = spawn('cargo', [
  'run', '-p', 'rust-agent-host', '--',
  '--mode', 'stdio',
  '--provider', 'deepseek',
  '--api-key', '$AGNES_API_KEY'
], {
  stdio: ['pipe', 'pipe', 'pipe']
});

// 通过 stdin/stdout 发送/接收 JSON-RPC 消息
hostProcess.stdin.write(JSON.stringify(initializeRequest) + '\n');
hostProcess.stdout.on('data', (data) => {
  const messages = data.toString().split('\n').filter(Boolean);
  for (const msg of messages) {
    const response = JSON.parse(msg);
    handleAcpMessage(response);
  }
});
```

### WebSocket 模式集成

适用于远程部署或多客户端共享场景。

```typescript
// TypeScript 示例：WebSocket 连接
const ws = new WebSocket('ws://127.0.0.1:9876/acp');

ws.onopen = () => {
  ws.send(JSON.stringify(initializeRequest));
};

ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  handleAcpMessage(message);
};
```

## ACP 协议交互流程

### 步骤 1：初始化

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocol_version": "0.14"
  }
}
```

响应中包含可用 Agent 列表：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocol_version": "0.14",
    "_meta": {
      "raf": {
        "version": "0.1.0",
        "agents": [
          {
            "id": "coding",
            "agent_type": "CodingAgent",
            "name": "coding",
            "description": "代码专家智能体",
            "has_subagents": false,
            "is_default": true
          },
          {
            "id": "dev-pipeline",
            "agent_type": "WorkflowAgent",
            "name": "dev-pipeline",
            "description": "图工作流 [dev-pipeline]: 8 节点, 8 条边",
            "has_subagents": true,
            "is_default": false
          }
        ]
      }
    }
  }
}
```

### 步骤 2：创建会话

指定目标 Agent（可选，不指定则使用默认 Agent）：

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "session/new",
  "params": {
    "_meta": {
      "raf": {
        "agent_id": "coding"
      }
    }
  }
}
```

### 步骤 3：发送提示（带每轮模型配置）

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "session/prompt",
  "params": {
    "session_id": "<从步骤2获取>",
    "prompt": [
      {
        "type": "text",
        "text": "请分析这段代码的性能瓶颈"
      }
    ],
    "_meta": {
      "raf": {
        "agent_id": "coding",
        "model_config": {
          "temperature": 0.2,
          "max_tokens": 4096,
          "thinking": true,
          "thinking_level": "high"
        }
      }
    }
  }
}
```

### 步骤 4：处理流式输出

Host 通过 `session/update` 通知推送流式输出：

```json
// 文本内容
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "session_id": "...",
    "update": {
      "type": "agent_message_chunk",
      "content": { "type": "text", "text": "分析结果：" }
    },
    "_meta": {
      "raf.agent_id": "coding",
      "raf.status": "executing"
    }
  }
}

// 思考内容（thinking=true 时）
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "session_id": "...",
    "update": {
      "type": "agent_thought_chunk",
      "content": { "type": "text", "text": "首先检查循环复杂度..." }
    }
  }
}

// 工具调用
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "session_id": "...",
    "update": {
      "type": "tool_call",
      "tool_call": {
        "id": "call_001",
        "title": "read_file",
        "status": "pending"
      }
    }
  }
}
```

### 步骤 5：接收完成响应

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "stop_reason": "end_turn"
  }
}
```

## 每轮模型配置详解

### 配置字段参考

| 字段 | 类型 | 范围 | 说明 |
|------|------|------|------|
| `temperature` | float | 0.0 - 2.0 | 低值=确定性，高值=创造性 |
| `max_tokens` | uint | > 0 | 单轮最大输出 token 数 |
| `thinking` | bool | true/false | 是否启用推理模式 |
| `thinking_level` | string | "high" / "max" | 推理深度（仅 thinking=true 时） |
| `context_window_tokens` | uint | > 0 | 覆盖压缩预算计算 |
| `max_output_tokens` | uint | > 0 | 覆盖压缩预算计算 |
| `model_id` | string | - | 模型 ID（仅元数据，不切换客户端） |

### 场景示例

**场景 1：快速问答（低延迟）**

```json
{
  "raf": {
    "model_config": {
      "temperature": 0.1,
      "max_tokens": 1024,
      "thinking": false
    }
  }
}
```

**场景 2：深度代码重构**

```json
{
  "raf": {
    "model_config": {
      "temperature": 0.3,
      "max_tokens": 8192,
      "thinking": true,
      "thinking_level": "high"
    }
  }
}
```

**场景 3：最大推理深度（复杂架构设计）**

```json
{
  "raf": {
    "model_config": {
      "temperature": 0.5,
      "max_tokens": 16384,
      "thinking": true,
      "thinking_level": "max"
    }
  }
}
```

### 客户端 UI 建议

IDE 客户端应在聊天窗口中提供模型配置选择器：

```
┌─────────────────────────────────────────────┐
│  聊天窗口                          [⚙ 配置]  │
├─────────────────────────────────────────────┤
│                                             │
│  [用户]: 请重构这个函数                      │
│                                             │
│  [Agent]: 好的，我来分析...                  │
│  ┌─ 思考过程 ──────────────────────┐        │
│  │ 首先检查函数复杂度...            │        │
│  └──────────────────────────────────┘       │
│                                             │
├─────────────────────────────────────────────┤
│  [输入消息...]                    [发送]     │
├─────────────────────────────────────────────┤
│  模型: agnes-2.0-flash | 温度: 0.3 |      │
│  思考: ✓ high | max_tokens: 8192            │
└─────────────────────────────────────────────┘
```

## 工作流 Agent 与 HITL

### Dev Pipeline 工作流

`dev-pipeline` 是 6 阶段编码工作流，支持人工确认（HITL）：

1. **需求分析** → 人工确认需求
2. **测试设计** → 人工确认测试用例
3. **架构设计** → 人工确认架构
4. **任务规划** → 人工确认任务分解
5. **编码实现**（alpha + beta 双路并行）
6. **回归测试 + 审查**

### HITL 确认流程

当工作流暂停等待确认时，Host 发送 `session/request_permission`：

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "session/request_permission",
  "params": {
    "session_id": "...",
    "tool_call": {
      "id": "hitl_p1_confirm",
      "title": "人工确认 — 节点 p1_confirm",
      "status": "pending"
    },
    "options": [
      {
        "id": "confirm",
        "title": "确认",
        "kind": "allow_once"
      },
      {
        "id": "revise",
        "title": "提供修改建议",
        "kind": "reject_once"
      }
    ],
    "_meta": {
      "raf.agent_id": "p1_confirm",
      "raf.halt_type": "human_confirmation"
    }
  }
}
```

### 客户端响应

**确认**：

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "result": {
    "outcome": {
      "type": "selected",
      "option_id": "confirm"
    }
  }
}
```

**提供修改建议**：

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "result": {
    "outcome": {
      "type": "selected",
      "option_id": "revise"
    },
    "_meta": {
      "raf": {
        "feedback": "请增加对边界条件的处理"
      }
    }
  }
}
```

## 标签化流式输出

工作流 Agent 的每个子 Agent 输出都带有 `_meta.raf.agent_id` 标签，客户端可据此渲染多 Agent 视图：

```json
{
  "method": "session/update",
  "params": {
    "session_id": "...",
    "update": { "type": "agent_message_chunk", "content": { "text": "需求分析完成" } },
    "_meta": {
      "raf.agent_id": "requirements-analyst",
      "raf.status": "executing"
    }
  }
}
```

客户端可根据 `raf.agent_id` 将输出路由到不同的 UI 区域：

```
┌─ requirements-analyst ─────────┐
│ 需求分析完成                    │
└────────────────────────────────┘
┌─ test-designer ────────────────┐
│ 正在设计测试用例...             │
└────────────────────────────────┘
```

## 取消会话

```json
{
  "jsonrpc": "2.0",
  "method": "session/cancel",
  "params": {
    "session_id": "..."
  }
}
```

Host 会设置取消令牌，Agent 在下一个工具循环迭代时中断。

## 错误处理

### Agent 未找到

如果 `_meta.raf.agent_id` 指定的 Agent 不存在，Host 返回空响应：

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "stop_reason": "end_turn"
  }
}
```

（日志中会记录 `warn: Agent not found`）

### 模型配置解析失败

如果 `_meta.raf.model_config` 格式错误，Host 静默回退到默认配置（日志中记录 debug 信息）。

## 完整客户端示例

以下是一个完整的 TypeScript 客户端示例：

```typescript
interface AcpClient {
  initialize(): Promise<AgentList>;
  createSession(agentId?: string): Promise<string>;
  prompt(
    sessionId: string,
    message: string,
    modelConfig?: ModelConfig
  ): AsyncIterator<SessionUpdate>;
  cancel(sessionId: string): Promise<void>;
}

interface ModelConfig {
  temperature?: number;
  max_tokens?: number;
  thinking?: boolean;
  thinking_level?: 'high' | 'max';
}

class RustAgentHostClient implements AcpClient {
  private nextId = 1;

  constructor(private transport: JsonRpcTransport) {}

  async initialize(): Promise<AgentList> {
    const response = await this.transport.request('initialize', {
      protocol_version: '0.14'
    });
    return response._meta?.raf?.agents ?? [];
  }

  async createSession(agentId?: string): Promise<string> {
    const params: any = {};
    if (agentId) {
      params._meta = { raf: { agent_id: agentId } };
    }
    const response = await this.transport.request('session/new', params);
    return response.session_id;
  }

  async *prompt(
    sessionId: string,
    message: string,
    modelConfig?: ModelConfig
  ): AsyncIterator<SessionUpdate> {
    const meta: any = {};
    if (modelConfig) {
      meta.raf = { model_config: modelConfig };
    }

    const promptId = this.nextId++;
    const updates = this.transport.sendNotificationAndListen(
      'session/prompt',
      {
        session_id: sessionId,
        prompt: [{ type: 'text', text: message }],
        _meta: meta
      },
      promptId
    );

    for await (const update of updates) {
      yield update;
      if (update.stop_reason) break;
    }
  }

  async cancel(sessionId: string): Promise<void> {
    await this.transport.sendNotification('session/cancel', {
      session_id: sessionId
    });
  }
}

// 使用示例
async function main() {
  const transport = new StdioTransport('cargo run -p rust-agent-host -- --mode stdio');
  const client = new RustAgentHostClient(transport);

  // 1. 初始化
  const agents = await client.initialize();
  console.log('可用 Agent:', agents);

  // 2. 创建会话
  const sessionId = await client.createSession('coding');

  // 3. 发送提示（带每轮模型配置）
  const stream = client.prompt(
    sessionId,
    '请重构这个函数',
    {
      temperature: 0.3,
      max_tokens: 8192,
      thinking: true,
      thinking_level: 'high'
    }
  );

  // 4. 处理流式输出
  for await (const update of stream) {
    if (update.type === 'agent_message_chunk') {
      process.stdout.write(update.content.text);
    } else if (update.type === 'agent_thought_chunk') {
      // 渲染思考内容
      console.log('\n[思考]', update.content.text);
    } else if (update.type === 'tool_call') {
      console.log('\n[工具调用]', update.tool_call.title);
    }
  }
}
```

## 下一步

- [14.1 Host Service 概述](overview.md)
- [14.2 ACP 协议与消息格式](acp-protocol.md)
- [14.3 传输层](transports.md)
- [14.7 IDE 集成与每轮模型配置](ide-integration.md)
