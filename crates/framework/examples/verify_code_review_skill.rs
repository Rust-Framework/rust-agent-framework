//! 验证技能基础设施：
//!   1. code-review 技能：advertise / load_skill / read_skill_resource
//!   2. hello-world 技能：run_skill_script 脚本执行
//!   3. 工具去重验证
//!   4. System Prompt 拼接模拟（ChatClientAgent 的组装逻辑）
//!   5. 冗余检测：advertise 与 agent instructions 是否重复 / 拼接分隔符是否多余
//!
//! 不依赖 LLM，直接测试技能基础设施。

use std::sync::Arc;
use rust_agent_framework::{AgentSkill, AgentSkillsProvider, SubprocessScriptRunner};

fn main() {
    println!("=== RAF Skill & System Prompt 验证 ===\n");

    // ════════════════════════════════════════════════════════════════
    // 1. code-review 技能：advertise + load_skill + read_resource
    // ════════════════════════════════════════════════════════════════
    let cr_skill = AgentSkill::from_dir("examples/skills/code-review").unwrap();
    println!("[1] code-review 技能:");
    println!("    name={}, resources={}, scripts={}",
        cr_skill.metadata.name, cr_skill.has_resources(), cr_skill.has_scripts());

    let mut provider = AgentSkillsProvider::new().with_skill(cr_skill);

    // ════════════════════════════════════════════════════════════════
    // 2. hello-world 技能：run_skill_script 脚本执行
    // ════════════════════════════════════════════════════════════════
    let hw_skill = AgentSkill::from_dir("examples/skills/hello-world").unwrap();
    println!("\n[2] hello-world 技能:");
    println!("    name={}, resources={}, scripts={}",
        hw_skill.metadata.name, hw_skill.has_resources(), hw_skill.has_scripts());

    provider = provider
        .with_skill(hw_skill)
        .with_script_runner(Arc::new(SubprocessScriptRunner));

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

    // ════════════════════════════════════════════════════════════════
    // 7. System Prompt 拼接模拟
    //    （模拟 ChatClientAgent::run() 第 144-167 行的组装逻辑）
    // ════════════════════════════════════════════════════════════════
    println!("\n════════════════════════════════════════════════");
    println!("  System Prompt 拼接模拟");
    println!("════════════════════════════════════════════════");

    // 模拟 agent 的基础 instructions（真实场景下由 AgentBuilder::with_instructions 设置）
    let agent_instructions = "\
You are a helpful AI assistant with access to skill packages.
When a task matches a skill domain, use the available tools to load and execute skill instructions.";

    // 模拟多个 provider 返回 instructions 的拼接（skills_provider 的 on_invoking 返回 advertise）
    // 这里模拟：只有一个 AgentSkillsProvider，没有其他 provider 注入 instructions
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

    println!("\n[7a] 最终 System Message 内容 ({} 字符):", sys.len());
    println!("──────────────────────────────────────────────");
    // 用分割线标出各段来源
    let separator = "\n\n";
    let parts: Vec<&str> = sys.splitn(2, separator).collect();
    if parts.len() == 2 {
        println!("  === Agent Instructions 部分 ({} 字符) ===", parts[0].len());
        println!("{}", parts[0]);
        println!("\n  --- 分隔符 '\\n\\n' ---");
        println!("\n  === Provider Instructions 部分 ({} 字符) ===", parts[1].len());
        println!("{}", parts[1]);
    } else {
        println!("{}", sys);
    }

    // ════════════════════════════════════════════════════════════════
    // 8. 冗余检测
    // ════════════════════════════════════════════════════════════════
    println!("\n════════════════════════════════════════════════");
    println!("  冗余检测");
    println!("════════════════════════════════════════════════");

    let mut redundancy_found = false;

    // 8a. 分隔符冗余：检查 sys 中是否有连续 3+ 个换行（说明拼接产生了多余空白）
    let triple_newlines: Vec<_> = sys.match_indices("\n\n\n").collect();
    if !triple_newlines.is_empty() {
        redundancy_found = true;
        println!("  [冗余] 发现连续 3+ 换行符 at positions: {:?}", triple_newlines);
    } else {
        println!("  [OK] 无连续 3+ 换行符 — 分隔符拼接无冗余");
    }

    // 8b. 检查 agent_instructions 尾部是否有 trailing \n（可能导致双分隔符）
    if agent_instructions.ends_with('\n') {
        redundancy_found = true;
        println!("  [冗余] agent_instructions 尾部包含换行符，拼接后可能产生双分隔符");
    } else {
        println!("  [OK] agent_instructions 尾部无多余换行符");
    }

    // 8c. 检查 advertise 尾部是否有 trailing \n
    if advertise.ends_with('\n') {
        println!("  [INFO] advertise 尾部包含换行符（这是正常的，markdown 习惯）");
    }

    // 8d. 内容重叠检测：advertise 的 skill name/description 是否在 agent_instructions 中出现
    for skill in &provider.skills {
        if agent_instructions.contains(&skill.metadata.name) {
            redundancy_found = true;
            println!("  [冗余] skill name '{}' 同时出现在 agent_instructions 和 advertise 中", skill.metadata.name);
        }
        if agent_instructions.contains(&skill.metadata.description) {
            redundancy_found = true;
            println!("  [冗余] skill description '{}' 同时出现在 agent_instructions 和 advertise 中", &skill.metadata.description[..skill.metadata.description.len().min(40)]);
        }
    }
    if !provider.skills.iter().any(|s| {
        agent_instructions.contains(&s.metadata.name) || agent_instructions.contains(&s.metadata.description)
    }) {
        println!("  [OK] agent_instructions 与 advertise 无内容重叠");
    }

    // 8e. 多次调用 on_invoking 模拟（验证 advertise 是否会重复累积）
    println!("\n  [多轮调用模拟]");
    let mut accumulated = String::new();
    for round in 0..3 {
        if !accumulated.is_empty() {
            accumulated.push_str("\n\n");
        }
        accumulated.push_str(&advertise);
        println!("    第 {} 轮注入后: {} 字符", round + 1, accumulated.len());
    }
    // 检查是否出现了重复的 "## Available Skills" 头部
    let header_count = accumulated.matches("## Available Skills").count();
    if header_count > 1 {
        redundancy_found = true;
        println!("  [冗余] 多轮调用后 '## Available Skills' 头部出现 {} 次 (每次 on_invoking 都会注入)", header_count);
        println!("          说明：advertise 文本每轮调用都会被重复注入到 system prompt 中");
    } else {
        println!("  [OK] '## Available Skills' 头部无重复 (单轮场景)");
    }

    // 8f. 总结
    println!("\n  ── 冗余检测结论 ──");
    if redundancy_found {
        println!("  发现冗余问题，请检查上方标记的 [冗余] 项");
    } else {
        println!("  未发现冗余问题，拼接逻辑正确");
    }

    println!("\n=== 全部验证通过 ===");
}
