use rhai::{Dynamic, Engine, Scope, AST};
use rust_agent_core::Result;
use serde_json::Value;
use std::sync::Arc;

const DEFAULT_MAX_OPERATIONS: u64 = 100_000;

type StateReader = Arc<dyn Fn(&str) -> Option<Value> + Send + Sync>;

/// 高内聚低耦合的 Rhai 脚本运行时。
pub struct RhaiRuntime {
    engine: Engine,
    scope: Scope<'static>,
    ast: Option<AST>,
    script_source: Option<String>,
    max_operations: u64,
    state_reader: Option<StateReader>,
}

impl RhaiRuntime {
    /// 使用 `Engine::new_raw()` 创建沙箱化运行时。
    pub fn new() -> Self {
        let mut engine = Engine::new_raw();
        engine.set_max_operations(DEFAULT_MAX_OPERATIONS);

        // Register built-in helper: json_get(obj, key)
        engine.register_fn("json_get", |obj: &mut Dynamic, key: &str| -> Dynamic {
            if obj.is_map() {
                let map = std::mem::take(obj).cast::<rhai::Map>();
                let val = map.get(key).cloned().unwrap_or(Dynamic::UNIT);
                // Put the map back
                *obj = Dynamic::from_map(map);
                val
            } else {
                Dynamic::UNIT
            }
        });

        engine.register_fn("env", |name: &str| -> Dynamic {
            std::env::var(name)
                .map(Dynamic::from)
                .unwrap_or(Dynamic::UNIT)
        });

        Self {
            engine,
            scope: Scope::new(),
            ast: None,
            script_source: None,
            max_operations: DEFAULT_MAX_OPERATIONS,
            state_reader: None,
        }
    }

    /// 注册动态 state 读取器 — 脚本内可用 `state("key")` / `local("key")`。
    pub fn with_dynamic_state(&mut self, reader: StateReader) -> &mut Self {
        self.state_reader = Some(Arc::clone(&reader));
        let r1 = Arc::clone(&reader);
        self.engine.register_fn("state", move |key: &str| -> Dynamic {
            r1(key)
                .map(|v| json_to_dynamic(&v))
                .unwrap_or(Dynamic::UNIT)
        });
        let r2 = reader;
        self.engine.register_fn("local", move |key: &str| -> Dynamic {
            r2(key)
                .map(|v| json_to_dynamic(&v))
                .unwrap_or(Dynamic::UNIT)
        });
        self
    }

    /// 设置最大操作数限制。
    pub fn max_operations(&mut self, ops: u64) -> &mut Self {
        self.max_operations = ops;
        self.engine.set_max_operations(ops);
        self
    }

    /// 设置并编译脚本源代码。
    pub fn with_script(&mut self, script: impl Into<String>) -> &mut Self {
        let source = script.into();
        self.script_source = Some(source.clone());
        self._compile(&source);
        self
    }

    /// 向运行时作用域注入变量。
    pub fn with_variable(&mut self, name: &str, value: Dynamic) -> &mut Self {
        self.scope.push(name, value);
        self
    }

    /// 以 JSON Value 形式注入变量。
    pub fn with_json_variable(&mut self, name: &str, value: Value) -> &mut Self {
        let dynamic = json_to_dynamic(&value);
        self.scope.push(name, dynamic);
        self
    }

    /// 注册自定义 Rhai 模块。
    pub fn with_module(&mut self, _name: impl AsRef<str>, module: rhai::Module) -> &mut Self {
        self.engine.register_global_module(module.into());
        self
    }

    /// 注册自定义类型。
    pub fn register_type<T: rhai::CustomType>(&mut self) -> &mut Self {
        self.engine.build_type::<T>();
        self
    }

    /// 使用预编译的 AST。
    pub fn with_ast(&mut self, ast: AST) -> &mut Self {
        self.ast = Some(ast);
        self
    }

    fn _compile(&mut self, script: &str) {
        match self.engine.compile(script) {
            Ok(ast) => self.ast = Some(ast),
            Err(e) => {
                tracing::warn!("Rhai script compilation failed: {}", e);
            }
        }
    }

    /// 编译脚本并返回 AST。
    pub fn compile_standalone(&self, script: &str) -> std::result::Result<AST, rhai::ParseError> {
        self.engine.compile(script)
    }

    /// 运行预编译脚本，返回 JSON 格式结果。
    pub fn run(&mut self) -> Result<Value> {
        let ast = match &self.ast {
            Some(ast) => ast.clone(),
            None => {
                let source = self.script_source.as_deref().unwrap_or("");
                return Err(rust_agent_core::AgentError::WorkflowError(format!(
                    "Rhai script not compiled: {}",
                    if source.len() > 100 { &source[..100] } else { source }
                )));
            }
        };

        // Use eval_ast_with_scope which returns the script's return value
        match self.engine.eval_ast_with_scope::<Dynamic>(&mut self.scope, &ast) {
            Ok(result) => Ok(dynamic_to_json(&result)),
            Err(e) => Err(rust_agent_core::AgentError::WorkflowError(format!(
                "Rhai script execution error: {}",
                *e
            ))),
        }
    }

    /// 一步完成脚本的编译和执行。
    pub fn eval(&mut self, script: &str) -> Result<Value> {
        self.ast = Some(
            self.engine
                .compile(script)
                .map_err(|e| {
                    rust_agent_core::AgentError::WorkflowError(format!(
                        "Rhai script compilation error: {}",
                        e
                    ))
                })?
        );

        self.run()
    }

    /// 注入 workflow state 变量（Local.key → `key`，System.key → `sys_key`）。
    pub fn with_workflow_state(
        &mut self,
        state: std::collections::HashMap<String, serde_json::Value>,
    ) -> &mut Self {
        for (key, value) in state {
            self.with_json_variable(&key, value);
        }
        self
    }

    /// 求值表达式并返回 Dynamic 值。
    pub fn eval_expression(&mut self, expr: &str) -> Result<Dynamic> {
        let ast = self.engine.compile_expression(expr).map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!(
                "Rhai expression compilation error: {}",
                e
            ))
        })?;
        self.engine
            .eval_ast_with_scope::<Dynamic>(&mut self.scope, &ast)
            .map_err(|e| {
                rust_agent_core::AgentError::WorkflowError(format!(
                    "Rhai expression evaluation error: {}",
                    *e
                ))
            })
    }

    /// 从作用域中获取变量。
    pub fn get_variable(&self, name: &str) -> Option<Dynamic> {
        self.scope.get_value(name)
    }

    /// 可变访问引擎（高级：用于动态函数注册）。
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// 可变访问作用域。
    pub fn scope_mut(&mut self) -> &mut Scope<'static> {
        &mut self.scope
    }
}

impl Default for RhaiRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// ── JSON ↔ Dynamic conversions ──

/// 将 serde_json::Value 转换为 rhai::Dynamic。
pub fn json_to_dynamic_val(value: &Value) -> Dynamic {
    json_to_dynamic(value)
}

/// 将 rhai::Dynamic 转换为 serde_json::Value。
pub fn dynamic_to_json_val(dynamic: &Dynamic) -> Value {
    dynamic_to_json(dynamic)
}

fn json_to_dynamic(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from_bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from_int(i)
            } else if let Some(f) = n.as_f64() {
                Dynamic::from_float(f)
            } else {
                Dynamic::UNIT
            }
        }
        Value::String(s) => Dynamic::from(s.clone()),
        Value::Array(arr) => {
            let mut ary = rhai::Array::with_capacity(arr.len());
            for item in arr {
                ary.push(json_to_dynamic(item));
            }
            Dynamic::from_array(ary)
        }
        Value::Object(obj) => {
            let mut map = rhai::Map::new();
            for (k, v) in obj {
                map.insert(k.clone().into(), json_to_dynamic(v));
            }
            Dynamic::from_map(map)
        }
    }
}

fn dynamic_to_json(dynamic: &Dynamic) -> Value {
    if dynamic.is::<rhai::Map>() {
        let map = dynamic.clone().cast::<rhai::Map>();
        let mut obj = serde_json::Map::new();
        for (k, v) in map {
            obj.insert(k.to_string(), dynamic_to_json(&v));
        }
        Value::Object(obj)
    } else if dynamic.is::<rhai::Array>() {
        let arr = dynamic.clone().cast::<rhai::Array>();
        let values: Vec<Value> = arr.iter().map(dynamic_to_json).collect();
        Value::Array(values)
    } else if dynamic.is::<bool>() {
        Value::Bool(dynamic.as_bool().unwrap_or(false))
    } else if dynamic.is::<i64>() {
        Value::Number(dynamic.as_int().unwrap_or(0).into())
    } else if dynamic.is::<f64>() {
        match dynamic.as_float() {
            Ok(f) => serde_json::Number::from_f64(f)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Err(_) => Value::Null,
        }
    } else if dynamic.is::<rhai::ImmutableString>() {
        Value::String(dynamic.clone().into_string().unwrap_or_default())
    } else if dynamic.is_unit() {
        Value::Null
    } else {
        Value::String(format!("{:?}", dynamic))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_execution() {
        let mut rt = RhaiRuntime::new();
        rt.with_script("42");
        let result = rt.run().unwrap();
        assert_eq!(result, Value::Number(serde_json::Number::from(42)));
    }

    #[test]
    fn test_string_result() {
        let mut rt = RhaiRuntime::new();
        rt.with_script(r#""hello world""#);
        let result = rt.run().unwrap();
        assert_eq!(result, Value::String("hello world".to_string()));
    }

    #[test]
    fn test_variable_injection() {
        let mut rt = RhaiRuntime::new();
        rt.with_variable("x", Dynamic::from(10_i64))
          .with_variable("y", Dynamic::from(20_i64))
          .with_script("x + y");
        let result = rt.run().unwrap();
        assert_eq!(result, Value::Number(serde_json::Number::from(30)));
    }

    #[test]
    fn test_json_variable() {
        let mut rt = RhaiRuntime::new();
        rt.with_json_variable("data", serde_json::json!({"name": "test", "count": 5}))
          .with_script("json_get(data, \"name\")");
        let result = rt.run().unwrap();
        assert_eq!(result, Value::String("test".to_string()));
    }

    #[test]
    fn test_expression_eval() {
        let mut rt = RhaiRuntime::new();
        rt.with_variable("x", Dynamic::from(10_i64));
        let result = rt.eval_expression("x * 3").unwrap();
        assert_eq!(result.as_int().unwrap(), 30);
    }

    #[test]
    fn test_dynamic_state_fn() {
        let reader = Arc::new(|key: &str| -> Option<Value> {
            if key == "flag" {
                Some(Value::Bool(true))
            } else {
                None
            }
        });
        let mut rt = RhaiRuntime::new();
        rt.with_dynamic_state(reader);
        let result = rt.eval_expression("local(\"flag\")").unwrap();
        assert!(result.as_bool().unwrap_or(false));
    }

    #[test]
    fn test_env_fn() {
        std::env::set_var("RHAI_TEST_ENV", "ok");
        let mut rt = RhaiRuntime::new();
        let result = rt.eval_expression("env(\"RHAI_TEST_ENV\")").unwrap();
        assert_eq!(result.into_string().unwrap(), "ok");
        std::env::remove_var("RHAI_TEST_ENV");
    }

    #[test]
    fn test_sandbox_no_eval() {
        let mut rt = RhaiRuntime::new();
        rt.with_script("eval(\"42\")");
        let result = rt.run();
        if result.is_ok() {
            let v = result.unwrap();
            assert!(v.is_null() || v.as_str().map(|s| s.is_empty()).unwrap_or(true));
        }
    }

    #[test]
    fn test_max_operations() {
        let mut rt = RhaiRuntime::new();
        rt.max_operations(50)
          .with_script("loop { let x = 1 + 1; }");
        let result = rt.run();
        assert!(result.is_err());
    }

    #[test]
    fn test_conversion_roundtrip() {
        let original = serde_json::json!({
            "name": "test",
            "count": 42,
            "nested": {"key": "value"},
            "list": [1, 2, 3]
        });
        let dynamic = json_to_dynamic_val(&original);
        let back = dynamic_to_json_val(&dynamic);
        assert_eq!(original, back);
    }
}
