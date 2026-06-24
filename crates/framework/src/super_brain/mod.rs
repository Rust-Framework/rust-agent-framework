//! Super Brain — persistent cross-session agent knowledge (OKF).
//!
//! | Layer | Module | Role |
//! |-------|--------|------|
//! | `okf` | Concept model, validation, audit, changelog |
//! | `curator` | Background write path (projection → LLM → files) |
//! | `search` | Retrieval backend traits (vector search future) |
//! | `provider` | `SuperBrainContextProvider` — Agent pipeline integration |
//! | `seed` | Template bootstrap |

mod okf;
mod curator;
mod search;

mod provider;
mod seed;

pub use okf::{
    append_consolidation_entry, scan_index_gaps, validate_super_brain, SuperBrainIssue,
    SuperBrainValidationReport, Concept, Frontmatter, SuperBrain, IndexGap,
};
pub use curator::{
    build_consolidation_context, build_turn_transcript, load_projection, project_messages,
    save_projection, wrap_super_brain_curator_client, ConsolidationJob, ConsolidationStatus,
    ConsolidationWorker, WorkerStats, PROJECTION_STATE_KEY,
};
pub use provider::SuperBrainContextProvider;
pub use search::{
    ConsolidationReport, FileMemoryStore, IEmbeddingModel, IMemoryStore, MemoryEntry,
    VectorMemoryStore,
};
pub use seed::seed_super_brain_dir;
