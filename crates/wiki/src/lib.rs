pub mod cache;
pub mod config;
pub mod default_schemas;
pub mod engine;
pub mod frontmatter;
pub mod graph;
pub mod index_manager;
pub mod index_schema;
pub mod ingest;
pub mod links;
pub mod markdown;
pub mod ops;
pub mod search;
pub mod slug;
pub mod space_builder;
pub mod spaces;
pub mod type_registry;
pub mod watch;

// Re-export core types
pub use config::{GlobalConfig, WikiConfig, ResolvedConfig};
pub use engine::{WikiEngine, EngineState, SpaceContext};
pub use slug::{Slug, WikiUri, ReadTarget};
