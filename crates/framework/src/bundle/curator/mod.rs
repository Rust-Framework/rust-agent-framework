mod agent;
pub(crate) mod chat;
mod context;
mod trace;
mod worker;

pub(crate) use agent::prepare_consolidation_messages;
pub(crate) use chat::CuratorChatClient;
pub use context::{
    build_consolidation_context, build_turn_transcript, load_projection, project_messages,
    save_projection, PROJECTION_STATE_KEY,
};
pub use trace::ConsolidationStatus;
pub use worker::{ConsolidationJob, ConsolidationWorker, WorkerStats};
