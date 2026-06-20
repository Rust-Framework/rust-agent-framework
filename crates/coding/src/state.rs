//! 工作流共享状态键与产物类型定义。
//!
//! 所有阶段通过 `IWorkflowContext::write_state` / `read_state` 共享产物。
use serde::Deserialize;

/// 工作流共享状态键 — 所有阶段通过 `IWorkflowContext` 共享。
pub mod state_keys {
    /// 阶段 1: 需求分析文档
    pub const REQUIREMENTS_DOC: &str = "requirements_doc";
    /// 阶段 1: 用户确认结果
    pub const USER_CONFIRMATION: &str = "user_confirmation";
    /// 阶段 2: 集成/冒烟测试用例
    pub const TEST_CASES: &str = "test_cases";
    /// 阶段 3: 架构设计文档
    pub const ARCHITECTURE_DOC: &str = "architecture_doc";
    /// 阶段 4a: 任务分解计划
    pub const TASK_PLAN: &str = "task_plan";
    /// 阶段 4b: coder-alpha 变更摘要
    pub const CODE_CHANGES_ALPHA: &str = "code_changes_alpha";
    /// 阶段 4b: coder-beta 变更摘要
    pub const CODE_CHANGES_BETA: &str = "code_changes_beta";
    /// 阶段 5: 回归测试结果
    pub const REGRESSION_RESULTS: &str = "regression_results";
    /// 阶段 6: 审查反馈
    pub const REVIEW_FEEDBACK: &str = "review_feedback";
    /// 反馈循环计数
    pub const ITERATION_COUNT: &str = "iteration_count";
}

/// 审查结论 — 由 reviewer 输出，用于 `exclusive_gateway` 条件判断。
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewVerdict {
    /// 全部预期是否达成
    pub passed: bool,
    /// 差异点列表
    #[serde(default)]
    pub discrepancies: Vec<String>,
    /// 根因分析（需求 / 设计 / 实现）
    #[serde(default)]
    pub root_cause: String,
    /// 修复建议
    #[serde(default)]
    pub fix_suggestions: Vec<String>,
}

impl ReviewVerdict {
    /// 从 reviewer 的 assistant 文本中解析审查结论。
    ///
    /// reviewer 被要求输出 JSON。此函数容忍 JSON 前后的 Markdown 围栏和说明文字。
    pub fn parse_from_text(text: &str) -> Option<Self> {
        // 尝试提取第一个 JSON 对象（容忍 ```json 围栏）
        let json_str = extract_json_object(text)?;
        serde_json::from_str::<ReviewVerdict>(&json_str).ok()
    }
}

/// 从可能包含 Markdown 围栏或说明文字的文本中提取第一个 `{...}` JSON 对象。
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '"' if !escape => in_string = !in_string,
            '\\' if in_string => escape = !escape,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + 1].to_string());
                }
            }
            _ => escape = false,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_verdict_from_plain_json() {
        let text =
            r#"{"passed": true, "discrepancies": [], "root_cause": "", "fix_suggestions": []}"#;
        let v = ReviewVerdict::parse_from_text(text).unwrap();
        assert!(v.passed);
    }

    #[test]
    fn parse_verdict_from_fenced_json() {
        let text = "审查结论：\n```json\n{\"passed\": false, \"discrepancies\": [\"缺失测试\"], \"root_cause\": \"实现\", \"fix_suggestions\": [\"补充测试\"]}\n```\n";
        let v = ReviewVerdict::parse_from_text(text).unwrap();
        assert!(!v.passed);
        assert_eq!(v.discrepancies.len(), 1);
    }

    #[test]
    fn parse_verdict_returns_none_for_invalid() {
        assert!(ReviewVerdict::parse_from_text("not json at all").is_none());
    }
}
