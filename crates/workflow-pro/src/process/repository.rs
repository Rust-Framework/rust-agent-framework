use std::collections::HashMap;
use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::Result;
use super::definition::ProcessDefinition;

#[async_trait]
pub trait IProcessRepository: Send + Sync { async fn save(&self, definition: ProcessDefinition) -> Result<()>; async fn find_by_id(&self, id: &str) -> Result<Option<ProcessDefinition>>; async fn find_all(&self) -> Result<Vec<ProcessDefinition>>; async fn delete(&self, id: &str) -> Result<bool>; }

pub struct InMemoryProcessRepository { definitions: Mutex<HashMap<String, ProcessDefinition>> }
impl InMemoryProcessRepository { pub fn new() -> Self { Self { definitions: Mutex::new(HashMap::new()) } } }
impl Default for InMemoryProcessRepository { fn default() -> Self { Self::new() } }

#[async_trait]
impl IProcessRepository for InMemoryProcessRepository {
    async fn save(&self, definition: ProcessDefinition) -> Result<()> { let id = definition.id.clone(); self.definitions.lock().insert(id, definition); Ok(()) }
    async fn find_by_id(&self, id: &str) -> Result<Option<ProcessDefinition>> { Ok(self.definitions.lock().get(id).cloned()) }
    async fn find_all(&self) -> Result<Vec<ProcessDefinition>> { Ok(self.definitions.lock().values().cloned().collect()) }
    async fn delete(&self, id: &str) -> Result<bool> { Ok(self.definitions.lock().remove(id).is_some()) }
}
