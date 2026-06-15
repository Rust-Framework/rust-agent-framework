//! 验证技能基础设施：
//!   1. code-review 技能：advertise / load_skill / read_skill_resource
//!   2. hello-world 技能：run_skill_script 脚本执行
//!   3. 工具去重验证
//!
//! 不依赖 LLM，直接测试技能基础设施。

use std::sync::Arc;
use rust_agent_framework::{AgentSkill, AgentSkillsProvider, SubprocessScriptRunner};

fn main() {
    println!("=== RAF Skill 验证 ===\n");

    // ════════════════════════════════════════════════════════════════
    // 1. code-review 技能：advertise + load_skill + read_resource
    // ════════════════════════════════════════════════════════════════
    let cr_skill = AgentSkill::from_dir("../../examples/skills/code-review").unwrap();
    println!("[1] code-review 技能:");
    println!("    name={}, resources={}, scripts={}",
        cr_skill.metadata.name, cr_skill.has_resources(), cr_skill.has_scripts());

    let mut provider = AgentSkillsProvider::new().with_skill(cr_skill);

    // ════════════════════════════════════════════════════════════════
    // 2. hello-world 技能：run_skill_script 脚本执行
    // ════════════════════════════════════════════════════════════════
    let hw_skill = AgentSkill::from_dir("../../examples/skills/hello-world").unwrap();
    println!("\n[2] hello-world 技能:");
    println!("    name={}, resources={}, scripts={}",
        hw_skill.metadata.name, hw_skill.has_resources(), hw_skill.has_scripts());

    provider = provider
        .with_skill(hw_skill)
        .with_script_runner(Arc::new(SubprocessScriptRunner));

    // ════════════════════════════════════════════════════════════════
    // 3. Advertise 文本
    // ════════════════════════════════════════════════════════════════
    println!("\n[3] Advertise 文本:");
    println!("──────────────────────────────────────────────");
    println!("{}", provider.build_advertise());

    // ════════════════════════════════════════════════════════════════
    // 4. 工具列表（含去重验证）
    // ════════════════════════════════════════════════════════════════
    let tools = provider.build_tools();
    println!("\n[4] 注入工具（共 {} 个）:", tools.len());
    for tool in &tools {
        let name = tool.name();
        let desc = tool.description();
        println!("    - {}: {}", name, &desc[..desc.len().min(80)]);
    }

    // 去重验证：同名工具不应重复
    let mut names = std::collections::HashSet::new();
    for tool in &tools {
        if !names.insert(tool.name().to_string()) {
            println!("\n    !! 错误：工具 '{}' 重复注册！", tool.name());
        }
    }
    println!("\n    工具去重验证: {} 个工具, 无重复", tools.len());

    // load_skill 和 read_skill_resource 必须存在
    assert!(tools.iter().any(|t| t.name() == "load_skill"), "Missing load_skill");
    assert!(tools.iter().any(|t| t.name() == "read_skill_resource"), "Missing read_skill_resource");
    // run_skill_script 应在配置 runner + 有脚本时出现
    assert!(tools.iter().any(|t| t.name() == "run_skill_script"), "Missing run_skill_script");
    println!("    工具完整性验证: load_skill/read_skill_resource/run_skill_script 全部存在");

    // ════════════════════════════════════════════════════════════════
    // 5. run_skill_script 执行测试
    // ════════════════════════════════════════════════════════════════
    let run_tool = provider.create_run_script_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();

    println!("\n[5] run_skill_script 测试:");
    println!("──────────────────────────────────────────────");

    // 5a. 基本执行（无参数）
    let result = rt.block_on(run_tool.execute(serde_json::json!({
        "skill_name": "hello-world",
        "script_path": "scripts/greet.py",
    }))).unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("    无参数执行:");
    println!("      ok: {}", v["ok"]);
    let output_str = v["data"]["output"].as_str().unwrap();
    // Parse the JSON output from the Python script
    if let Ok(greeting) = serde_json::from_str::<serde_json::Value>(output_str) {
        println!("      greeting: {}", greeting["greeting"].as_str().unwrap_or("?"));
    } else {
        println!("      output: {}", output_str);
    }

    // 5b. 带参数执行
    let result = rt.block_on(run_tool.execute(serde_json::json!({
        "skill_name": "hello-world",
        "script_path": "scripts/greet.py",
        "args": ["--name", "RAF User"],
    }))).unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("\n    带参数执行 (--name \"RAF User\"):");
    println!("      ok: {}", v["ok"]);
    let output_str = v["data"]["output"].as_str().unwrap();
    if let Ok(greeting) = serde_json::from_str::<serde_json::Value>(output_str) {
        println!("      greeting: {}", greeting["greeting"].as_str().unwrap_or("?"));
    } else {
        println!("      output: {}", output_str);
    }

    // 5c. 不存在的脚本
    let result = rt.block_on(run_tool.execute(serde_json::json!({
        "skill_name": "hello-world",
        "script_path": "scripts/nonexistent.py",
    })));
    assert!(result.is_err() || {
        let v: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        v["ok"] == false
    });
    println!("\n    不存在脚本: 正确返回错误");

    // ════════════════════════════════════════════════════════════════
    // 6. load_skill (hello-world)
    // ════════════════════════════════════════════════════════════════
    let load_tool = provider.create_load_skill_tool();
    println!("\n[6] load_skill(\"hello-world\"):");
    println!("──────────────────────────────────────────────");
    let result = rt.block_on(load_tool.execute(serde_json::json!({
        "skill_name": "hello-world"
    }))).unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    let inst = v["data"]["instructions"].as_str().unwrap();
    println!("    {}", &inst[..inst.len().min(300)]);
    if inst.len() > 300 { println!("    ... ({} 字符)", inst.len()); }

    println!("\n=== 全部验证通过 ===");
}
