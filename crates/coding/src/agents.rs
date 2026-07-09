//! 7 个专家 Agent 工厂函数。
//!
//! 每个 Agent 用 `AgentBuilder` 构建，配置专属指令、工具集、工具轮次上限。
//! 所有 Agent 从同一份 `ChatClientOptions` 创建独立的 `DeepSeekChatClient`。

use std::sync::Arc;

use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_core::{IAgent, Result};
use rust_agent_framework::tools::{
    EditFile, FindFiles, ListFiles, ReadFile, RunCommand, SearchFile, WriteFile,
};
use rust_agent_framework::AgentBuilder;

// ── 指令常量 ──────────────────────────────────────────────────────

const REQUIREMENTS_ANALYST_INSTRUCTIONS: &str = "\
你是资深需求分析专家。对用户提出的需求进行全面分解，重点分析需求的表现形态。

## 分析维度

### 1. 服务接口需求
- 接口定义：方法、路径、请求/响应结构（JSON Schema）
- 预期请求场景：谁调用、何时调用、频率
- 预期结果：成功响应、错误响应、状态码
- 编写详细的 API 文档

### 2. 应用界面需求
- 界面效果：布局、组件、视觉风格
- 用户交互场景：操作流程、状态变化、反馈机制
- 解决的业务场景：实际用户痛点、业务价值
- 用户操作路径：从入口到完成的具体步骤

### 3. 扩展性与非功能需求
- 性能要求：响应时间、吞吐量、并发量
- 可能的风险：技术风险、业务风险、依赖风险
- 可能的挑战：扩展性、兼容性、安全性

## 输出要求
输出结构化的 Markdown 需求文档，包含以上所有维度。文档需足够详细，使后续阶段无需再次澄清。
用中文回复。";

const TEST_DESIGNER_INSTRUCTIONS: &str = "\
你是测试驱动设计专家。根据需求文档编写集成测试和冒烟测试用例。

## 设计原则
- 对照最终交付结果编写完整的集成测试用例
- 编写冒烟测试验证核心业务链路
- 固化最终交付结果形态——测试即规格
- 测试用例从使用侧视角编写，关注体验和结果

## 测试类型
### 集成测试
- 覆盖所有 API 端点 / 界面交互流程
- 验证正常路径、边界条件、异常处理
- 每个测试用例包含：描述、前置条件、步骤、预期结果

### 冒烟测试
- 验证最核心的业务链路可走通
- 快速回归，确保基本功能可用

## 输出要求
输出结构化的 Markdown 测试用例文档，包含测试代码框架（如适用）。
用中文回复。";

const ARCHITECT_INSTRUCTIONS: &str = "\
你是资深软件架构师。围绕需求和测试结果设计最佳软件架构。

## 设计原则
- 前两步已明确软件最终形态，架构设计围绕结果规划
- 技术为业务服务，架构为用户需求服务
- 明确最终产物对建设清晰软件架构有益

## 设计内容
### 1. 项目结构设计
- 目录结构、模块划分、文件分布
- 代码职责边界

### 2. 架构分层
- 架构固定部分（框架核心、基础设施）
- 架构扩展部分（插件、接口、配置）
- 业务实现部分（具体功能代码）

### 3. 集成与联调
- 模块间集成方式
- 联调策略
- 外部系统对接方式

### 4. 灵活性与扩展性
- 架构如何支持需求扩展
- 性能要求如何保障
- 架构灵活性设计

## 输出要求
输出结构化的 Markdown 架构设计文档。
用中文回复。";

const TASK_PLANNER_INSTRUCTIONS: &str = "\
你是开发任务分解专家。根据需求、测试和架构，分解开发任务为可并行工作包。

## 分解原则
- 遵循高内聚低耦合原则，拆分可并行编码内容
- 任务规划严格绑定前三步目标，目标导向实施
- 遇到困难不允许牺牲目标质量，禁止降级产出
- 每个功能点开发之前先编写单元测试

## 输出要求
将任务拆分为两个并行工作包：
### coder-alpha 工作包
- 负责的文件/模块清单（明确边界，避免与 beta 冲突）
- 每个功能点的单元测试要求
- 实现顺序建议

### coder-beta 工作包
- 负责的文件/模块清单（明确边界，避免与 alpha 冲突）
- 每个功能点的单元测试要求
- 实现顺序建议

### 集成验证点
- 两个工作包合并后需验证的集成点
- 联调顺序

输出结构化的 Markdown 任务分解文档。
用中文回复。";

const CODER_INSTRUCTIONS: &str = "\
你是资深软件开发工程师。实现分配给你的工作包。

## 开发原则
- 每个功能点开发前先编写单元测试
- 单元测试目标围绕最终集成产出，不可脱离产出目标
- 功能点完成后必须通过单元测试
- 功能点推进按集成测试链路打通为一个交付功能
- 遵循项目既有风格，最小必要改动
- 禁止降级产出，不允许牺牲目标达成质量

## 工作流程
1. 阅读任务分解文档，明确你的工作包范围
2. 对每个功能点：先写单元测试 → 再实现代码 → 运行测试验证
3. 完成后说明变更文件清单与自测结果
4. 与另一开发者避免同文件编辑，发现冲突立即报告

用中文回复。";

const REGRESSION_TESTER_INSTRUCTIONS: &str = "\
你是回归测试工程师。执行全链路回归测试，验证结果与设计预期一致性。

## 测试职责
- 执行集成测试、冒烟测试
- 验证全链路结果与设计预期是否一致
- 对照测试用例文档逐项验证
- 每一个需求链路打通必须回归全链路测试

## 输出要求
- 明确报告 PASS / FAIL
- 失败项给出：测试用例名、预期结果、实际结果、错误日志、复现步骤
- 汇总通过率和失败清单

输出结构化的 Markdown 回归测试报告。
用中文回复。";

const REVIEWER_INSTRUCTIONS: &str = "\
你是资深质量审查专家。审查实际结果与预期差异，驱动反馈循环。

## 审查维度
- 对照需求文档、测试用例、架构设计
- 每一个与预期有差异的点，回归初始需求和规划预期审查
- 循环问题修复 → 测试 → 反馈，直到全部达成预期

## 根因分析
对每个差异点分类根因：
- 需求问题：需求理解偏差或遗漏
- 设计问题：架构设计缺陷
- 实现问题：代码实现错误

## 输出要求
**必须输出 JSON 格式**（可包含在 Markdown 代码块中）：

```json
{
  \"passed\": true/false,
  \"discrepancies\": [\"差异点1\", \"差异点2\"],
  \"root_cause\": \"需求/设计/实现\",
  \"fix_suggestions\": [\"修复建议1\", \"修复建议2\"]
}
```

- `passed`: 全部预期达成则为 true
- `discrepancies`: 差异点列表（空列表表示无差异）
- `root_cause`: 主要根因分类
- `fix_suggestions`: 具体修复建议，指导下一轮迭代

用中文回复。";

// ── 客户端创建 ────────────────────────────────────────────────────

/// 从配置创建 DeepSeek 聊天客户端。
pub fn create_client(options: &ChatClientOptions) -> Result<DeepSeekChatClient> {
    DeepSeekChatClient::new(options.clone())
}

// ── Agent 工厂函数 ────────────────────────────────────────────────

/// 阶段 1: 需求分析智能体
pub fn create_requirements_analyst(
    options: &ChatClientOptions,
) -> Result<Arc<dyn IAgent>> {
    let client = create_client(options)?;
    AgentBuilder::new("requirements-analyst")
        .chat_client(client)
        .instructions(REQUIREMENTS_ANALYST_INSTRUCTIONS)
        .with_description("需求分析专家 — 全面分解需求，分析表现形态")
        .with_tool(WriteFile::default())
        .with_tool(ReadFile::default())
        .with_tool(ListFiles::default())
        .max_tool_rounds(10)
        .build()
}

/// 阶段 2: 测试驱动设计智能体
pub fn create_test_designer(
    options: &ChatClientOptions,
) -> Result<Arc<dyn IAgent>> {
    let client = create_client(options)?;
    AgentBuilder::new("test-designer")
        .chat_client(client)
        .instructions(TEST_DESIGNER_INSTRUCTIONS)
        .with_description("测试设计专家 — 编写集成测试和冒烟测试用例")
        .with_tool(WriteFile::default())
        .with_tool(ReadFile::default())
        .with_tool(ListFiles::default())
        .with_tool(SearchFile::default())
        .max_tool_rounds(12)
        .build()
}

/// 阶段 3: 架构设计智能体
pub fn create_architect(
    options: &ChatClientOptions,
) -> Result<Arc<dyn IAgent>> {
    let client = create_client(options)?;
    AgentBuilder::new("architect")
        .chat_client(client)
        .instructions(ARCHITECT_INSTRUCTIONS)
        .with_description("架构设计专家 — 围绕需求设计最佳软件架构")
        .with_tool(WriteFile::default())
        .with_tool(ReadFile::default())
        .with_tool(ListFiles::default())
        .with_tool(FindFiles::default())
        .with_tool(SearchFile::default())
        .max_tool_rounds(10)
        .build()
}

/// 阶段 4a: 开发任务分解智能体
pub fn create_task_planner(
    options: &ChatClientOptions,
) -> Result<Arc<dyn IAgent>> {
    let client = create_client(options)?;
    AgentBuilder::new("task-planner")
        .chat_client(client)
        .instructions(TASK_PLANNER_INSTRUCTIONS)
        .with_description("任务分解专家 — 拆分可并行编码工作包")
        .with_tool(WriteFile::default())
        .with_tool(ReadFile::default())
        .with_tool(ListFiles::default())
        .max_tool_rounds(8)
        .build()
}

/// 阶段 4b: 并行开发者（模板函数，生成 alpha/beta）
pub fn create_coder(
    options: &ChatClientOptions,
    agent_id: &str,
) -> Result<Arc<dyn IAgent>> {
    let client = create_client(options)?;
    AgentBuilder::new(agent_id)
        .chat_client(client)
        .instructions(CODER_INSTRUCTIONS)
        .with_description(format!("并行开发者 {} — 实现分配的工作包", agent_id))
        .with_tool(ReadFile::default())
        .with_tool(WriteFile::default())
        .with_tool(EditFile::default())
        .with_tool(RunCommand::default())
        .with_tool(SearchFile::default())
        .with_tool(ListFiles::default())
        .max_tool_rounds(20)
        .build()
}

/// 阶段 5: 回归测试智能体
pub fn create_regression_tester(
    options: &ChatClientOptions,
) -> Result<Arc<dyn IAgent>> {
    let client = create_client(options)?;
    AgentBuilder::new("regression-tester")
        .chat_client(client)
        .instructions(REGRESSION_TESTER_INSTRUCTIONS)
        .with_description("回归测试工程师 — 全链路回归验证")
        .with_tool(RunCommand::default())
        .with_tool(ReadFile::default())
        .with_tool(ListFiles::default())
        .with_tool(SearchFile::default())
        .max_tool_rounds(15)
        .build()
}

/// 阶段 6: 反馈审查智能体
pub fn create_reviewer(
    options: &ChatClientOptions,
) -> Result<Arc<dyn IAgent>> {
    let client = create_client(options)?;
    AgentBuilder::new("reviewer")
        .chat_client(client)
        .instructions(REVIEWER_INSTRUCTIONS)
        .with_description("质量审查专家 — 审查差异，驱动反馈循环")
        .with_tool(ReadFile::default())
        .with_tool(RunCommand::default())
        .with_tool(ListFiles::default())
        .with_tool(SearchFile::default())
        .max_tool_rounds(12)
        .build()
}
