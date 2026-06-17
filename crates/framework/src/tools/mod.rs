pub mod path_guard;
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
pub mod load_skill;
pub mod read_skill_resource;
pub mod run_skill_script;

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
pub use load_skill::LoadSkillTool;
pub use read_skill_resource::ReadSkillResourceTool;
pub use run_skill_script::RunSkillScriptTool;

use std::path::PathBuf;

use rust_agent_core::ToolRegistry;

/// Register all built-in file-system tools at once.
///
/// Each tool is constructed with the given `base_dir` for relative path resolution.
/// Absolute paths passed by the LLM bypass the base_dir.
pub fn register_all(registry: &mut ToolRegistry, base_dir: impl Into<PathBuf>) {
    let base_dir = base_dir.into();
    registry.register(ReadFile::new(&base_dir));
    registry.register(WriteFile::new(&base_dir));
    registry.register(EditFile::new(&base_dir));
    registry.register(ListFiles::new(&base_dir));
    registry.register(InspectFile::new(&base_dir));
    registry.register(MakeDirectory::new(&base_dir));
    registry.register(RemovePath::new(&base_dir));
    registry.register(MoveFile::new(&base_dir));
    registry.register(FindFiles::new(&base_dir));
    registry.register(SearchFile::new(&base_dir));
    registry.register(RunCommand::new(&base_dir));
}

/// Helper: build a success response.
fn ok_response(data: serde_json::Value) -> String {
    serde_json::json!({"ok": true, "data": data}).to_string()
}

/// Helper: build an error response.
fn err_response(msg: &str) -> String {
    serde_json::json!({"ok": false, "data": null, "error": msg}).to_string()
}
