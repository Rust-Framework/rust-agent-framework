pub mod cache;
pub mod config;
pub mod confidence;
pub mod conflict;
pub mod context_provider;
pub mod default_schemas;
pub mod engine;
pub mod forgetting;
pub mod frontmatter;
pub mod gate;
pub mod governance;
pub mod graph;
pub mod hybrid;
pub mod index_manager;
pub mod index_schema;
pub mod ingest;
pub mod links;
pub mod markdown;
pub mod memory;
pub mod ops;
pub mod search;
pub mod slug;
pub mod space_builder;
pub mod spaces;
pub mod type_registry;
pub mod vector;
pub mod watch;

// Re-export core types
pub use config::{GlobalConfig, WikiConfig, ResolvedConfig};
pub use engine::{from_repo, WikiEngine, EngineState, SpaceContext};
pub use slug::{Slug, WikiUri, ReadTarget};

// Re-export v2 modules' public API
pub use confidence::{compute as compute_confidence, compute_confidence as dyn_confidence, ConfidenceInput, ConfidenceBreakdown};
pub use conflict::{detect as detect_conflicts, propose_resolution, Conflict, ConflictKind, Resolution, ResolutionAction};
pub use forgetting::{decay as decay_confidence, decay_from_frontmatter, DecayConfig, DecayResult, DecayStatus};
pub use gate::{evaluate as gate_evaluate, GateConfig, GateContext, GateDecision};
pub use governance::{GovernanceConfig, GovernanceScheduler, GovernanceTask, GovernanceReport};
pub use hybrid::{hybrid_search, render_hybrid_llms, HybridParams, HybridResult, SourceRank};
pub use memory::{MemoryStore, MemoryConfig, MemoryTier, MemoryStats, Observation, EpisodicEntry, SemanticEntry, ProceduralEntry};
pub use vector::{VectorIndex, VectorHit};
pub use context_provider::WikiContextProvider;
