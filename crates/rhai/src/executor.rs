use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;

use rust_agent_workflow::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

use crate::runtime::RhaiRuntime;

/// RhaiExecutor — 将 Rhai 脚本适配为 workflow 的 [`IExecutor`]。
///
/// 作为工作流节点，接收上游消息，通过 Rhai 脚本执行动态逻辑，
/// 结果以 [`HandlerResult::Messages`] 传递给下游节点。
///
/// # 脚本内置变量
///
/// 每次执行时，作用域中自动注入以下变量：
///
/// | 名称 | 类型 | 说明 |
/// |------|------|------|
/// | 自定义（`input_var`） | Dynamic | 上游消息内容（JSON Value） |
/// | `node_id` | String | 当前执行节点 ID |
/// | `context` | Map | 当前工作流上下文状态快照（所有 state） |
/// | `_meta` | Map | 执行元信息（node_id 等） |
///
/// # 脚本可调用的内置函数
///
/// | 函数 | 说明 |
/// |------|------|
/// | `emit_text(msg)` | 推送流式文本进度事件 |
/// | `emit_custom(key, value)` | 推送自定义进度事件 |
/// | `set_output(key, value)` | 回写状态到上下文（延迟提交） |
///
/// # 示例
///
/// ```ignore
/// use rust_agent_rhai::RhaiExecutor;
///
/// let executor = RhaiExecutor::new(
///     "data_transformer",
///     r#"
///         let result = #{name: input.name.to_upper(), count: input.count + 1};
///         emit_text("转换完成");
///         result
///     "#,
///     "input",
/// );
/// ```
pub struct RhaiExecutor {
    id: String,
    input_var: String,
    runtime: Arc<Mutex<RhaiRuntime>>,
}

impl RhaiExecutor {
    /// 创建新的 RhaiExecutor。
    ///
    /// # 参数
    /// - `id`: 执行器唯一标识
    /// - `script`: Rhai 脚本源文本
    /// - `input_var`: 上游消息绑定到的变量名（在脚本中通过此名称访问输入）
    pub fn new(id: impl Into<String>, script: impl Into<String>, input_var: impl Into<String>) -> Self {
        let script_str = script.into();
        let mut runtime = RhaiRuntime::new();
        runtime.with_script(script_str.clone());

        Self {
            id: id.into(),
            runtime: Arc::new(Mutex::new(runtime)),
            input_var: input_var.into(),
        }
    }

    /// 使用预配置的 RhaiRuntime 创建执行器。
    pub fn with_runtime(
        id: impl Into<String>,
        mut runtime: RhaiRuntime,
        script: impl Into<String>,
        input_var: impl Into<String>,
    ) -> Self {
        let script_str = script.into();
        runtime.with_script(script_str.clone());

        Self {
            id: id.into(),
            runtime: Arc::new(Mutex::new(runtime)),
            input_var: input_var.into(),
        }
    }
}

#[async_trait]
impl IExecutor for RhaiExecutor {
    fn id(&self) -> &str {
        &self.id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("serde_json::Value")]
    }

    fn send_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("serde_json::Value")]
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        // 1. 提取输入数据
        let input_value = extract_input(message);

        // 2. 预加载上下文状态快照（异步操作，执行前完成）
        let node_id = ctx.current_node_id().to_string();
        let context_snapshot = load_context_snapshot(&*ctx).await?;

        // 3. 获取 runtime 锁，注入执行期变量和回调，执行脚本，然后立即释放锁
        let (result, pending_writes) = {
            let mut runtime = self.runtime.lock();

            // 注入变量
            runtime.with_json_variable(&self.input_var, input_value);
            runtime.scope_mut().push("node_id", node_id.clone());
            runtime.with_json_variable("context", context_snapshot.clone());
            runtime.with_json_variable("_meta", serde_json::json!({
                "node_id": node_id,
            }));

            // 注入进度回调
            let pc1 = progress.clone();
            runtime.engine_mut().register_fn("emit_text", move |msg: &str| {
                let _ = pc1.send(NodeProgress::TextDelta(msg.to_string()));
            });

            let pc2 = progress.clone();
            runtime.engine_mut().register_fn("emit_custom", move |key: &str, value: &str| {
                let val = serde_json::from_str::<serde_json::Value>(value)
                    .unwrap_or(serde_json::Value::String(value.to_string()));
                let _ = pc2.send(NodeProgress::Custom {
                    key: key.to_string(),
                    value: val,
                });
            });

            // 注入 set_output
            let output_writes: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(vec![]));
            let writes = output_writes.clone();
            runtime.engine_mut().register_fn("set_output", move |key: &str, value: &str| {
                let val = serde_json::from_str::<serde_json::Value>(value)
                    .unwrap_or(serde_json::Value::String(value.to_string()));
                writes.lock().push((key.to_string(), val));
            });

            // 执行脚本
            let result = runtime.run()?;

            // 收集 output_writes（在锁释放前）
            let pending = output_writes.lock().clone();

            (result, pending)
        }; // runtime MutexGuard dropped here

        // 4. 回写 output_writes 到上下文（异步，不再持有 runtime 锁）
        for (key, value) in &pending_writes {
            ctx.write_state(key, value.clone()).await?;
        }

        // 5. 包装输出
        Ok(HandlerResult::Messages(vec![Arc::new(result)]))
    }
}

/// 从上下文加载所有已知状态到 JSON 快照。
///
/// 当前实现尝试读取一些常见 key，未来可扩展为遍历所有已知 key。
async fn load_context_snapshot(ctx: &dyn IWorkflowContext) -> Result<serde_json::Value> {
    // 尝试读取常见状态 key，构建 snapshot map
    // 未来可通过 ctx 的 iterator 或约定前缀实现完整的快照
    let common_keys = ["input", "result", "config", "state", "data"];
    let mut snapshot = serde_json::Map::new();

    for key in &common_keys {
        if let Some(val) = ctx.read_state(key).await? {
            snapshot.insert(key.to_string(), val);
        }
    }

    Ok(serde_json::Value::Object(snapshot))
}

/// 从 message 中提取 serde_json::Value
fn extract_input(message: Arc<dyn std::any::Any + Send + Sync>) -> serde_json::Value {
    if let Some(val) = message.downcast_ref::<serde_json::Value>() {
        return val.clone();
    }
    if let Some(s) = message.downcast_ref::<String>() {
        return serde_json::Value::String(s.clone());
    }
    if let Some(s) = message.downcast_ref::<&str>() {
        return serde_json::Value::String(s.to_string());
    }
    serde_json::Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_creation() {
        let executor = RhaiExecutor::new("test_node", "input + 1", "input");
        assert_eq!(executor.id(), "test_node");
        assert_eq!(executor.accepted_types().len(), 1);
    }

    #[test]
    fn test_extract_input_json() {
        let msg: Arc<dyn std::any::Any + Send + Sync> = Arc::new(serde_json::json!({"key": "value"}));
        let result = extract_input(msg);
        assert_eq!(result, serde_json::json!({"key": "value"}));
    }

    #[test]
    fn test_extract_input_string() {
        let msg: Arc<dyn std::any::Any + Send + Sync> = Arc::new("hello".to_string());
        let result = extract_input(msg);
        assert_eq!(result, serde_json::Value::String("hello".to_string()));
    }

    #[test]
    fn test_extract_input_unknown() {
        let msg: Arc<dyn std::any::Any + Send + Sync> = Arc::new(42_i32);
        let result = extract_input(msg);
        assert_eq!(result, serde_json::Value::Null);
    }
}
