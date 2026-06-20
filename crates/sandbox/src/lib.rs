//! # rust-agent-sandbox
//!
//! 代码沙箱扩展 crate — 实现 [`ICodeSandbox`]，提供 `code_interpreter` 工具。
//!
//! 核心 crate 仅定义 trait；本 crate 承载进程隔离等具体后端，避免污染 core/framework。

pub mod code_interpreter;
pub mod container;
#[cfg(feature = "docker")]
pub mod docker;
pub mod process;
pub mod wasm;

pub use code_interpreter::CodeInterpreterTool;
pub use container::ContainerSandbox;
#[cfg(feature = "docker")]
pub use docker::DockerSandbox;
pub use process::ProcessSandbox;
pub use wasm::WasmSandbox;
