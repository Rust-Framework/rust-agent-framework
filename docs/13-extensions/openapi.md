# 13.10 OpenAPI 工具

`rust-agent-openapi` 从 OpenAPI 3.x 规范解析 HTTP 操作并实例化为 `ITool`，支持 path/query/header 参数、Bearer 安全方案和可选的响应 JSON Schema 校验。

## 核心类型

| 类型 | 说明 |
|------|------|
| `OpenApiSpec` | 解析后的规范对象 |
| `OpenApiHttpTool` | 单个操作的 HTTP 工具实现 |
| `OpenApiToolResolver` | 从 URL 或 JSON 字符串解析工具 |
| `OpenApiToolConfig` | `spec_url`、`operation_id`、`tool_name`、`base_url` |

## 代码注册

```rust
use rust_agent_openapi::{OpenApiToolConfig, OpenApiToolResolver};

let tool = OpenApiToolResolver::resolve(&OpenApiToolConfig {
    tool_name: "get_pet".into(),
    spec_url: "file://./petstore.yaml".into(),
    operation_id: Some("getPetById".into()),
    base_url: None,
})
.await?;

AgentBuilder::new("api-agent")
    .chat_client(client)
    .with_tool(tool)
    .build()?;
```

## 响应 Schema 校验

启用 `validate` feature 后，工具执行结果会按 operation 的 `200`/`201`/`204` 响应 schema 校验（使用 `jsonschema`）：

```toml
rust-agent-openapi = { version = "0.1", features = ["validate"] }
rust-agent-decl = { version = "0.1", features = ["openapi", "openapi-validate"] }
```

校验失败时 `ToolResult.ok` 为 `false`，结果 JSON 含 `schema_valid: false` 和 `schema_error` 字段。

## 声明式配置

```yaml
tools:
  - kind: openapi
    name: get_pet
    specUrl: file://./openapi/petstore.yaml
    operationId: getPetById
```

需 `rust-agent-decl` 的 `openapi` feature；CLI 默认启用 `openapi-validate`。

## 规范解析能力

- `$ref` 解析（components/schemas、parameters、responses）
- 嵌套 object/array schema 深度展开
- Path 参数替换、query/header 参数注入
- `securitySchemes` Bearer token（从工具参数或环境注入）
