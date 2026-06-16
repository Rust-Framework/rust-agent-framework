use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::{ITool, Result};

use crate::runtime::RhaiRuntime;

/// RhaiTool — 将 Rhai 脚本封装为 [`ITool`]，供智能体通过 ToolRegistry 调用。
///
/// Agent 识别到需要调用此工具时，将参数以 JSON 形式传入，
/// 工具内部将参数注入作用域后执行脚本，返回执行结果。
///
/// # 脚本内置变量
///
/// | 名称 | 类型 | 说明 |
/// |------|------|------|
/// | `args` | Dynamic (Map) | 工具调用参数（JSON 对象） |
///
/// # 示例
///
/// ```ignore
/// use rust_agent_rhai::RhaiTool;
/// use rust_agent_core::ITool;
///
/// let tool = RhaiTool::new(
///     "calculate",
///     "执行自定义计算逻辑",
///     serde_json::json!({
///         "type": "object",
///         "properties": {
///             "x": {"type": "number"},
///             "y": {"type": "number"}
///         }
///     }),
///     r#"#{result: args.x + args.y, formula: "x + y"}"#,
/// );
/// ```
pub struct RhaiTool {
    name: String,
    description: String,
    parameters: serde_json::Value,
    runtime: Arc<Mutex<RhaiRuntime>>,
}

impl RhaiTool {
    /// 创建新的 RhaiTool。
    ///
    /// # 参数
    /// - `name`: 工具名称（Agent 通过此名称匹配）
    /// - `description`: 工具描述（用于 Agent function calling）
    /// - `parameters`: JSON Schema 格式的参数定义
    /// - `script`: Rhai 脚本内容
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
        script: impl Into<String>,
    ) -> Self {
        let script_str = script.into();
        let mut runtime = RhaiRuntime::new();
        runtime.with_script(script_str.clone());

        Self {
            name: name.into(),
            description: description.into(),
            parameters,
            runtime: Arc::new(Mutex::new(runtime)),
        }
    }

    /// 使用预配置的 RhaiRuntime 创建工具。
    ///
    /// 当需要提前注入自定义模块、函数或初始变量时使用。
    pub fn with_runtime(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
        mut runtime: RhaiRuntime,
        script: impl Into<String>,
    ) -> Self {
        let script_str = script.into();
        runtime.with_script(script_str.clone());

        Self {
            name: name.into(),
            description: description.into(),
            parameters,
            runtime: Arc::new(Mutex::new(runtime)),
        }
    }

    /// 从 Rhai 脚本文件创建工具。
    ///
    /// 直接从文件系统加载 `.rhai` 脚本。
    /// 适用于将脚本独立管理和版本控制。
    pub fn from_script_file(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
        script_path: impl AsRef<std::path::Path>,
    ) -> std::result::Result<Self, String> {
        let script = std::fs::read_to_string(script_path.as_ref())
            .map_err(|e| format!("读取脚本文件失败: {}", e))?;
        Ok(Self::new(name, description, parameters, script))
    }
}

#[async_trait]
impl ITool for RhaiTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        let mut runtime = self.runtime.lock();

        // 将 arguments 注入为 args 变量
        runtime.with_json_variable("args", arguments);

        // 执行脚本
        let result = runtime.run()?;

        // 结果转为 JSON 字符串
        Ok(serde_json::to_string(&result).unwrap_or_else(|_| "null".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime as TokioRuntime;

    #[test]
    fn test_tool_creation() {
        let tool = RhaiTool::new(
            "hello",
            "say hello",
            serde_json::json!({"type": "object", "properties": {}}),
            r#""hello world""#,
        );
        assert_eq!(tool.name(), "hello");
        assert_eq!(tool.description(), "say hello");
    }

    #[test]
    fn test_tool_execute() {
        let tool = RhaiTool::new(
            "add",
            "add two numbers",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "x": {"type": "number"},
                    "y": {"type": "number"}
                }
            }),
            r#"args.x + args.y"#,
        );

        let rt = TokioRuntime::new().unwrap();
        let result = rt.block_on(tool.execute(serde_json::json!({"x": 3, "y": 4})));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "7");
    }

    #[test]
    fn test_tool_execute_complex() {
        let tool = RhaiTool::new(
            "process",
            "process data",
            serde_json::json!({"type": "object"}),
            r#"
                let name = args.name;
                let items = args.items;
                let total = items[0].price + items[1].price + items[2].price;
                #{name: name, total: total, count: 3}
            "#,
        );

        let rt = TokioRuntime::new().unwrap();
        let result = rt.block_on(tool.execute(serde_json::json!({
            "name": "order_1",
            "items": [
                {"price": 10},
                {"price": 20},
                {"price": 30}
            ]
        })));
        assert!(result.is_ok(), "error: {:?}", result);
        let val: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(val["name"], "order_1");
        assert_eq!(val["total"], 60);
        assert_eq!(val["count"], 3);
    }
}
