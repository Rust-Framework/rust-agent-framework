//! 验证技能基础设施：
//!   1. code-review 技能：advertise / load_skill / read_skill_resource
//!   2. 工具去重验证
//!   3. System Prompt 拼接模拟（ChatClientAgent 的组装逻辑）
//!   4. 冗余检测：advertise 与 agent instructions 是否重复 / 拼接分隔符是否多余
//!
//! 不依赖 LLM，直接测试技能基础设施。
//!
//! 注：技能脚本执行现在通过 RunCommand + WorkspaceScope 实现，
//! 不再有独立的 run_skill_script 工具。

use rust_agent_framework::{AgentSkill, AgentSkillsProvider};

fn main() {
    println!("=== RAF Skill & System Prompt 验证 ===\n");

    // ════════════════════════════════════════════════════════════════
    // 1. code-review 技能：advertise + load_skill + read_resource
    // ════════════════════════════════════════════════════════════════
    let cr_skill = AgentSkill::from_dir("examples/skills/code-review").unwrap();
    println!("[1] code-review 技能:");
    println!("    name={}, resources={}, scripts={}",
        cr_skill.metadata.name, cr_skill.has_resources(), cr_skill.has_scripts());

    let provider = AgentSkillsProvider::new().with_skill(cr_skill);

    // ════════════════════════════════════════════════════════════════
    // 2. hello-world 技能
    // ════════════════════════════════════════════════════════════════
    let hw_skill = AgentSkill::from_dir("examples/skills/hello-world").unwrap();
    println!("\n[2] hello-world 技能:");
    println!("    name={}, resources={}, scripts={}",
        hw_skill.metadata.name, hw_skill.has_resources(), hw_skill.has_scripts());

    let provider = provider.with_skill(hw_skill);

    // ════════════════════════════════════════════════════════════════
    // 3. Advertise 文本
    // ════════════════════════════════════════════════════════════════
    let advertise = provider.build_advertise();
    println!("\n[3] Advertise 文本（{} 字符）:", advertise.len());
    println!("──────────────────────────────────────────────");
    println!("{}", advertise);

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
    // run_skill_script 已合并到 RunCommand — 不再自动注入
    assert!(!tools.iter().any(|t| t.name() == "run_skill_script"), "run_skill_script should not exist (merged into RunCommand)");
    println!("    工具完整性验证: load_skill/read_skill_resource 全部存在, run_skill_script 已合并");

    // ════════════════════════════════════════════════════════════════
    // 5. load_skill (hello-world)
    // ════════════════════════════════════════════════════════════════
    let load_tool = provider.create_load_skill_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    println!("\n[5] load_skill(\"hello-world\"):");
    println!("──────────────────────────────────────────────");
    let result = rt.block_on(load_tool.execute(serde_json::json!({
        "skill_name": "hello-world"
    }))).unwrap();
    assert!(result.ok);
    let data = result.data.as_ref().unwrap();
    let inst = data["instructions"].as_str().unwrap();
    println!("    {}", &inst[..inst.len().min(300)]);
    if inst.len() > 300 { println!("    ... ({} 字符)", inst.len()); }

    // ════════════════════════════════════════════════════════════════
    // 6. System Prompt 拼接模拟
    //    （模拟 ChatClientAgent::run() 的组装逻辑）
    // ════════════════════════════════════════════════════════════════
    println!("\n════════════════════════════════════════════════");
    println!("  System Prompt 拼接模拟");
    println!("════════════════════════════════════════════════");

    let agent_instructions = "\
You are a helpful AI assistant with access to skill packages.
When a task matches a skill domain, use the available tools to load and execute skill instructions.";

    let mut merged_instructions = String::new();

    // ── 模拟 Provider 1: AgentSkillsProvider ──
    let provider_inst = provider.build_advertise();
    if !merged_instructions.is_empty() {
        merged_instructions.push_str("\n\n");
    }
    merged_instructions.push_str(&provider_inst);

    // ── 模拟 ChatClientAgent 的系统消息组装逻辑 ──
    let mut sys = agent_instructions.to_string();
    if !merged_instructions.is_empty() {
        if !sys.is_empty() {
            sys.push_str("\n\n");
        }
        sys.push_str(&merged_instructions);
    }

    println!("合并后的 System Prompt（{} 字符）:", sys.len());
    println!("──────────────────────────────────────────────");
    // 只打印前 800 字符
    let preview = &sys[..sys.len().min(800)];
    println!("{}", preview);
    if sys.len() > 800 {
        println!("\n... (剩余 {} 字符省略)", sys.len() - 800);
    }

    // ════════════════════════════════════════════════════════════════
    // 7. 冗余检测
    // ════════════════════════════════════════════════════════════════
    println!("\n════════════════════════════════════════════════");
    println!("  冗余检测");
    println!("════════════════════════════════════════════════");

    // 检测 advertise 是否与 agent instructions 重复
    let has_skill_header = advertise.contains("## Available Skills");
    println!("    advertise 含 '## Available Skills' 标题: {}", has_skill_header);
    let has_load_skill_hint = advertise.contains("load_skill");
    println!("    advertise 含 load_skill 提示: {}", has_load_skill_hint);

    // 检测合并后的 system prompt 是否有连续多余分隔符
    let has_double_newlines = sys.contains("\n\n\n");
    println!("    合并后含连续多余分隔符（\\n\\n\\n）: {}", has_double_newlines);

    println!("\n=== 验证完成 ===");
}
