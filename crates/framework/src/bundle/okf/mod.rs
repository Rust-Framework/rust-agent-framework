mod model;
mod validate;
mod changelog;
mod audit;

pub use changelog::append_consolidation_entry;
pub use audit::{format_index_gaps, scan_index_gaps, IndexGap};
pub use model::{Concept, Frontmatter, KnowledgeBundle};
pub use validate::{validate_bundle, BundleIssue, BundleValidationReport};
