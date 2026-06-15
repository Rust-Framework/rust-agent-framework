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

/// ToolRegistry — manages tool registration and lookup following MAF.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ITool>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: HashMap::new() } }

    pub fn register(&mut self, tool: impl ITool + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn register_arc(&mut self, tool: Arc<dyn ITool>) {
        self.tools.insert(tool.name().to_string(), tool);
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
