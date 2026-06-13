# rust-agent-llm

LLM 工具与辅助层，提供 prompt 工程和模型元数据管理。

## 功能定位

为 LLM 交互提供辅助工具，不涉及实际 API 调用，聚焦于调用前的准备和配置。

- **PromptTemplate**: prompt 模板引擎，支持 `{{variable}}` 插值语法，分离 system/user 消息组装
- **ModelInfo**: 模型元数据（provider、context window、max tokens、能力标记），内置常见模型预设

## 专属职责

- 管理 prompt 模板的定义、渲染和变量替换
- 维护模型能力元数据（是否支持 tool、streaming 等）
- 为 `rust-agent-client` 和 `rust-agent-framework` 提供模型选择依据

## 不做什么

- 不发起 HTTP 请求或 API 调用
- 不实现 `IChatClient`
- 不做 token 计数（可由外部库扩展）
- 不做 embedding 或向量操作
