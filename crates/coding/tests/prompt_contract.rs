//! 提示词契约验证 — 静态断言每个 Agent 的提示词文本符合工程契约。
//!
//! 不依赖 LLM，确定性验证提示词常量包含必备章节：
//! - 阶段定位（上下游）
//! - 工作区探索引导
//! - 思考框架（CoT/Plan-and-Solve/ReAct）
//! - 自检清单（Self-Verification）
//! - 产物契约（持久化路径 + 格式约束）
//!
//! 运行方式：
//! ```bash
//! cargo test -p rust-agent-coding --test prompt_contract -- --nocapture
//! ```

use rust_agent_client::ChatClientOptions;
use rust_agent_coding::agents::{
    ARCHITECT_INSTRUCTIONS, CODER_INSTRUCTIONS, REGRESSION_TESTER_INSTRUCTIONS,
    REQUIREMENTS_ANALYST_INSTRUCTIONS, REVIEWER_INSTRUCTIONS, TASK_PLANNER_INSTRUCTIONS,
    TEST_DESIGNER_INSTRUCTIONS,
};
use tempfile::tempdir;

/// 辅助：断言提示词包含若干必备子串。
fn assert_contains(name: &str, prompt: &str, required: &[&str]) {
    for &needle in required {
        assert!(
            prompt.contains(needle),
            "[{}] 提示词应包含「{}」，实际内容:\n{}",
            name,
            needle,
            prompt
        );
    }
}

/// 辅助：断言提示词以阶段定位开头（包含「6 阶段开发流水线」）。
fn assert_stage_header(name: &str, prompt: &str) {
    assert_contains(name, prompt, &["6 阶段开发流水线", "上游", "下游"]);
}

/// 辅助：断言提示词包含产物契约章节。
fn assert_artifact_contract(name: &str, prompt: &str) {
    assert_contains(name, prompt, &["产物契约", ".coding/", "用中文回复"]);
}

/// 辅助：断言提示词包含自检清单。
fn assert_self_verification(name: &str, prompt: &str) {
    assert_contains(name, prompt, &["自检清单", "[ ]"]);
}

/// 辅助：断言提示词禁止对话性语句。
fn assert_no_conversational(name: &str, prompt: &str) {
    assert_contains(name, prompt, &["不要对话性语句"]);
}

#[test]
fn test_requirements_analyst_prompt_contract() {
    let p = REQUIREMENTS_ANALYST_INSTRUCTIONS;
    assert_stage_header("需求分析师", p);
    assert_contains("需求分析师", p, &["工作区探索", "ListFiles", "ReadFile"]);
    assert_contains("需求分析师", p, &["思考框架", "以终为始"]);
    assert_contains("需求分析师", p, &["验收标准"]);
    assert_self_verification("需求分析师", p);
    assert_artifact_contract("需求分析师", p);
    assert_no_conversational("需求分析师", p);
    assert_contains("需求分析师", p, &["requirements.md"]);
}

#[test]
fn test_test_designer_prompt_contract() {
    let p = TEST_DESIGNER_INSTRUCTIONS;
    assert_stage_header("测试设计师", p);
    assert_contains("测试设计师", p, &["工作区探索", "技术栈", "WriteFile"]);
    assert_contains("测试设计师", p, &["测试即规格"]);
    assert_contains("测试设计师", p, &["产物 1", "产物 2"]);
    assert_self_verification("测试设计师", p);
    assert_artifact_contract("测试设计师", p);
    assert_contains("测试设计师", p, &["test_cases.md"]);
}

#[test]
fn test_architect_prompt_contract() {
    let p = ARCHITECT_INSTRUCTIONS;
    assert_stage_header("架构师", p);
    assert_contains("架构师", p, &["工作区探索", "ListFiles", "ReadFile"]);
    assert_contains("架构师", p, &["[alpha]", "[beta]", "[shared]"]);
    assert_contains("架构师", p, &["模块归属标注", "无文件重叠"]);
    assert_self_verification("架构师", p);
    assert_artifact_contract("架构师", p);
    assert_contains("架构师", p, &["architecture.md"]);
}

#[test]
fn test_task_planner_prompt_contract() {
    let p = TASK_PLANNER_INSTRUCTIONS;
    assert_stage_header("任务分解师", p);
    assert_contains("任务分解师", p, &["alpha", "beta", "工作包"]);
    assert_contains("任务分解师", p, &["集成验证点"]);
    assert_contains("任务分解师", p, &["不要重复定义单元测试"]);
    assert_self_verification("任务分解师", p);
    assert_artifact_contract("任务分解师", p);
    assert_contains("任务分解师", p, &["task_plan.md"]);
}

#[test]
fn test_coder_prompt_contract() {
    let p = CODER_INSTRUCTIONS;
    assert_stage_header("coder", p);
    assert_contains("coder", p, &["ReAct", "TDD"]);
    assert_contains("coder", p, &["WriteFile", "EditFile", "RunCommand"]);
    assert_contains("coder", p, &["完成标准", "变更清单"]);
    assert_contains("coder", p, &["禁止降级产出", "#[ignore]"]);
    assert_self_verification("coder", p);
    // coder 的产物是变更清单（不是 .coding 文件），但仍需中文回复
    assert_contains("coder", p, &["用中文回复"]);
}

#[test]
fn test_regression_tester_prompt_contract() {
    let p = REGRESSION_TESTER_INSTRUCTIONS;
    assert_stage_header("回归测试师", p);
    assert_contains("回归测试师", p, &["RunCommand", "ListFiles"]);
    assert_contains("回归测试师", p, &["PASS", "FAIL"]);
    assert_contains("回归测试师", p, &["exit code"]);
    assert_contains("回归测试师", p, &["失败项报告格式"]);
    assert_self_verification("回归测试师", p);
    assert_artifact_contract("回归测试师", p);
    assert_contains("回归测试师", p, &["regression.md"]);
}

#[test]
fn test_reviewer_prompt_contract() {
    let p = REVIEWER_INSTRUCTIONS;
    assert_stage_header("审查专家", p);
    assert_contains("审查专家", p, &["判定优先级", "首要依据", "回归测试报告"]);
    assert_contains("审查专家", p, &["合法 JSON"]);
    // JSON 示例必须使用字面量 true/false（非占位符）
    assert_contains("审查专家", p, &["字面量", "占位符"]);
    assert_contains("审查专家", p, &["fix_suggestions"]);
    assert_contains("审查专家", p, &["具体到文件"]);
    assert_self_verification("审查专家", p);
    assert_artifact_contract("审查专家", p);
    assert_contains("审查专家", p, &["review.md"]);
}

/// 验证 reviewer 提示词中的 JSON 示例能被 ReviewVerdict 正确解析。
/// 这是端到端契约的关键——提示词示例必须是合法且可解析的 JSON。
#[test]
fn test_reviewer_json_example_parseable() {
    let p = REVIEWER_INSTRUCTIONS;
    // 提取 JSON 代码块
    let start = p.find("```json").expect("应有 ```json 代码块");
    let json_start = p[start..].find('{').expect("应有 {") + start;
    let json_str = {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        let mut end = 0;
        for (i, ch) in p[json_start..].char_indices() {
            match ch {
                '"' if !esc => in_str = !in_str,
                '\\' if in_str => esc = !esc,
                '{' if !in_str => depth += 1,
                '}' if !in_str => {
                    depth -= 1;
                    if depth == 0 {
                        end = json_start + i + 1;
                        break;
                    }
                }
                _ => esc = false,
            }
        }
        &p[json_start..end]
    };

    let verdict = rust_agent_coding::ReviewVerdict::parse_from_text(json_str)
        .expect("提示词中的 JSON 示例应能被 ReviewVerdict 解析");
    assert!(!verdict.passed, "示例应为未通过状态");
    assert!(!verdict.discrepancies.is_empty(), "应有差异点");
    assert!(!verdict.fix_suggestions.is_empty(), "应有修复建议");
    assert!(
        verdict.fix_suggestions.iter().any(|s| s.contains(".rs")),
        "修复建议应具体到文件"
    );
}

/// 验证所有 agent 工厂在 mock options 下能成功构建（工具配置无类型错误）。
#[test]
fn test_all_agent_factories_build() {
    let options = ChatClientOptions {
        api_base: "https://mock".into(),
        api_key: "mock-key".into(),
        model: "mock".into(),
        ..Default::default()
    };
    let workspace = tempdir().expect("tempdir");
    let root = workspace.path();

    rust_agent_coding::agents::create_requirements_analyst(&options, root).expect("analyst");
    rust_agent_coding::agents::create_test_designer(&options, root).expect("designer");
    rust_agent_coding::agents::create_architect(&options, root).expect("architect");
    rust_agent_coding::agents::create_task_planner(&options, root).expect("planner");
    rust_agent_coding::agents::create_coder(&options, root, "coder-alpha").expect("coder-alpha");
    rust_agent_coding::agents::create_coder(&options, root, "coder-beta").expect("coder-beta");
    rust_agent_coding::agents::create_regression_tester(&options, root).expect("tester");
    rust_agent_coding::agents::create_reviewer(&options, root).expect("reviewer");
}
