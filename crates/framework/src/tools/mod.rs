pub mod read_file;
pub mod write_file;
pub mod edit_file;
pub mod list_files;
pub mod inspect_file;
pub mod make_directory;
pub mod remove_path;
pub mod move_file;
pub mod find_files;
pub mod search_file;
pub mod run_command;

pub use read_file::ReadFile;
pub use write_file::WriteFile;
pub use edit_file::EditFile;
pub use list_files::ListFiles;
pub use inspect_file::InspectFile;
pub use make_directory::MakeDirectory;
pub use remove_path::RemovePath;
pub use move_file::MoveFile;
pub use find_files::FindFiles;
pub use search_file::SearchFile;
pub use run_command::RunCommand;

use rust_agent_core::ToolRegistry;

/// Register all built-in tools at once.
pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(ReadFile);
    registry.register(WriteFile);
    registry.register(EditFile);
    registry.register(ListFiles);
    registry.register(InspectFile);
    registry.register(MakeDirectory);
    registry.register(RemovePath);
    registry.register(MoveFile);
    registry.register(FindFiles);
    registry.register(SearchFile);
    registry.register(RunCommand);
}

/// Helper: build a success response.
fn ok_response(data: serde_json::Value) -> String {
    serde_json::json!({"ok": true, "data": data}).to_string()
}

/// Helper: build an error response.
fn err_response(msg: &str) -> String {
    serde_json::json!({"ok": false, "data": null, "error": msg}).to_string()
}
