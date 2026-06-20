#![cfg(all(feature = "yaml", feature = "sandbox"))]

//! ExecuteCode 工作流动作编译测试

use rust_agent_decl::compiler::{compile_workflow, registry::CompileRegistry};
use rust_agent_decl::workflow_decl::WorkflowAgentData;

#[test]
fn workflow_execute_code_compiles() {
    let yaml = r#"
trigger:
  kind: OnConversationStart
  id: start
  actions:
    - kind: ExecuteCode
      id: run_py
      code: print(1)
      language: python
      sandbox:
        backend: process
      output:
        result: Local.code_out
sandbox:
  backend: process
  timeout_secs: 30
"#;
    let data: WorkflowAgentData = serde_yaml::from_str(yaml).expect("parse workflow yaml");
    let mut registry = CompileRegistry::new();
    let graph = compile_workflow(&data, &mut registry).expect("compile workflow");
    assert!(!graph.nodes().is_empty());
}
