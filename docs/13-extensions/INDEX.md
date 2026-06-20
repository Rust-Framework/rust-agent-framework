# 第 13 章：扩展能力

RAF 提供了丰富的扩展能力生态，包括网络搜索、RAG、Wiki、代码沙箱、OpenAPI 工具、技能系统、Rhai 脚本和 MCP 集成。

## 扩展生态概览

```mermaid
graph TB
    subgraph "RAF 核心"
        IA[IAgent]
        IT[ITool]
        ICP[IContextProvider]
    end

    subgraph "扩展能力"
        WS[WebSearch / WebFetch]
        RAG[RAG 管道]
        WK[Wiki 引擎]
        SK[技能系统]
        RH[Rhai 脚本]
        SB[代码沙箱]
        OA[OpenAPI 工具]
        MM[SkillMemory]
    end

    WS -->|实现| IT
    WS -->|实现| ICP
    RAG -->|提供 traits| IT
    WK -->|Agent 集成| IA
    SK -->|注入| ICP
    RH -->|实现| IT
    RH -->|实现| IExecutor
    MM -->|实现| ICP

    IT --> IA
    ICP --> IA
```

## 章节目录

| 小节 | 标题 | 内容概要 |
|------|------|---------|
| [13.1](overview.md) | 扩展体系概述 | 扩展机制、ITool 与 IContextProvider 插件点 |
| [13.2](websearch.md) | 网络搜索 | WebSearch、WebFetch、多后端、反检测、内容清洗 |
| [13.3](rag.md) | 检索增强生成（RAG） | 文档加载、分块、嵌入、向量存储、检索全管道 |
| [13.4](wiki.md) | Wiki 知识引擎 | 空间、全文搜索、概念图、Agent 集成 |
| [13.5](skills.md) | Agent 技能系统 | SKILL.md、AgentSkill、动态/目录加载 |
| [13.6](rhai-scripts.md) | Rhai 脚本引擎 | RhaiRuntime、RhaiExecutor、RhaiTool |
| [13.7](memory.md) | SkillMemory 记忆系统 | 后台记忆整合、MemoryAgent、ConsolidationWorker |
| [13.8](mcp.md) | MCP 协议集成 | McpClient、McpTool、McpContextProvider、工具/资源/提示词操作 |
| [13.9](sandbox.md) | 代码沙箱 | ICodeSandbox、ProcessSandbox、DockerSandbox、CodeInterpreterTool |
| [13.10](openapi.md) | OpenAPI 工具 | OpenApiHttpTool、Schema 校验、声明式 kind: openapi |

## 快速导航

- **想让 Agent 搜索互联网？** → [13.2 网络搜索](websearch.md)
- **想让 Agent 检索本地文档？** → [13.3 RAG 管道](rag.md)
- **想构建知识库？** → [13.4 Wiki 引擎](wiki.md)
- **想为 Agent 添加可复用技能？** → [13.5 技能系统](skills.md)
- **想用脚本扩展 Agent？** → [13.6 Rhai 脚本](rhai-scripts.md)
- **想让 Agent 拥有持久记忆？** → [13.7 记忆系统](memory.md)
- **想让 Agent 执行代码？** → [13.9 代码沙箱](sandbox.md)
- **想从 OpenAPI 规范生成工具？** → [13.10 OpenAPI 工具](openapi.md)
- **想让 Agent 集成 MCP 工具？** → [13.8 MCP 协议集成](mcp.md)

---

## 上一步

← [第 12 章：业务流程编排引擎](../12-process-engine/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[第 14 章：宿主服务](../14-host-service/overview.md)** 以将 Agent 部署为远程服务，理解 ACP 协议、双传输模式和标签化流式输出。
