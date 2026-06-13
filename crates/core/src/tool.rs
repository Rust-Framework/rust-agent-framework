use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::Result;

/// Tool interface following MAF's tool abstraction.
#[async_trait]
pub trait ITool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute(&self, arguments: serde_json::Value) -> Result<String>;
}

/// AIFunction — internal ITool implementation for dynamic tool registration.
///
/// Users should prefer the `#[tool]` macro for defining tools.
/// This type is pub(crate) to avoid exposing the complex handler signature.
#[allow(dead_code)]
pub(crate) struct AIFunction {
    name: String,
    description: String,
    parameters_schema: serde_json::Value,
    handler: Box<
        dyn Fn(
                serde_json::Value,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>>
            + Send
            + Sync,
    >,
}

#[allow(dead_code)]
impl AIFunction {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String>> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: description.into(),
            parameters_schema,
            handler: Box::new(move |args| Box::pin(handler(args))),
        }
    }
}

#[async_trait]
impl ITool for AIFunction {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn parameters_schema(&self) -> serde_json::Value { self.parameters_schema.clone() }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        (self.handler)(arguments).await
    }
}

/// ToolRegistry — manages tool registration and lookup following MAF.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ITool>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: HashMap::new() } }

    pub fn register(&mut self, tool: impl ITool + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn ITool>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&Arc<dyn ITool>> {
        self.tools.values().collect()
    }

    pub fn len(&self) -> usize { self.tools.len() }
    pub fn is_empty(&self) -> bool { self.tools.is_empty() }
}

impl Default for ToolRegistry {
    fn default() -> Self { Self::new() }
}
