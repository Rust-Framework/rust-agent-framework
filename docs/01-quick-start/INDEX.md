# 第 1 章：快速入门

本章面向初次接触 Rust Agent Framework (RAF) 的开发者，帮助你从零开始搭建环境、创建第一个智能体，并理解框架的核心概念。

## 章节导航

1. **[环境安装](./installation.md)** — 安装 Rust 工具链，配置 `Cargo.toml` workspace 依赖，创建依赖 RAF 各 crate 的项目。
2. **[第一个智能体](./first-agent.md)** — 使用 `AgentBuilder` 构建智能体，配置系统指令，注册工具，处理流式输出和会话管理。
3. **[核心概念](./core-concepts.md)** — 详解 Agent、Tool、ChatClient、Session、ContextProvider、Message、Streaming 七大核心概念的职责和协作关系。
4. **[内置工具概览](./builtin-tools-intro.md)** — 快速浏览全部 14 个内置工具：`read_file`、`write_file`、`edit_file`、`list_files` 等，了解基本用法模式。

## 阅读建议

- **如果你是第一次接触 RAF**：按顺序阅读，从"环境安装"到"内置工具概览"。每节都包含可运行的代码示例。
- **如果你已有 Rust 基础、想快速跑起来**：直接跳到"第一个智能体"，代码示例开箱即用。
- **如果你需要评估 RAF**：先读"核心概念"了解架构设计决策，再根据需要深入后续章节。

## 预备知识

- 熟悉 Rust 语言基础（所有权、`async/await`、`#[tokio::main]`）
- 了解 LLM API 的基本概念（system prompt、message role、tool calling）
- 有一个 LLM API Key（DeepSeek 或 OpenAI 兼容均可）

---

## 下一步

阅读完本章后，建议继续阅读 **[核心架构](../02-core-architecture/layered-design.md)** 以深入了解 RAF 的四层架构设计、类型系统、消息模型和 Crate 依赖关系。
