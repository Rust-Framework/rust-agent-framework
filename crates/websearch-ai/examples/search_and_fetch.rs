//! CLI 测试：输入搜索关键词 → web_search 获取结果 → 自动 web_fetch 抓取页面内容
//!
//! 用法:
//!   cargo run --example search_and_fetch -- <keyword> [count] [result_index]
//!
//! 示例:
//!   cargo run --example search_and_fetch -- Rust编程语言 3 0
//!
//! 参数:
//!   keyword      - 搜索关键词（必填）
//!   count        - 返回结果数量（可选，默认 3，最大 10）
//!   result_index - 要抓取的第几条结果，从 0 开始（可选，默认 0）

use rust_agent_websearch::{WebFetch, WebSearch};
use std::env;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: cargo run --example search_and_fetch -- <keyword> [count] [result_index]");
        eprintln!("示例: cargo run --example search_and_fetch -- Rust编程语言 3 0");
        std::process::exit(1);
    }

    let query = &args[1];
    let count: i64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3)
        .clamp(1, 10);
    let result_idx: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

    // ==================== 第一步: web_search ====================
    println!("╔══════════════════════════════════════════════╗");
    println!("║           第一步: Web Search                 ║");
    println!("╚══════════════════════════════════════════════╝");
    println!("查询: {}", query);
    println!("数量: {}\n", count);

    let raw = WebSearch.call(query.to_string(), Some(count)).await;
    let json: serde_json::Value =
        serde_json::from_str(&raw).expect("解析 web_search 返回的 JSON 失败");

    if json["ok"] == false {
        eprintln!("搜索失败!");
        eprintln!("错误: {}", json["error"].as_str().unwrap_or("未知错误"));
        if let Some(suggestion) = json["suggestion"].as_str() {
            eprintln!("建议: {}", suggestion);
        }
        std::process::exit(1);
    }

    let results = &json["data"]["results"];
    let total = results.as_array().map(|a| a.len()).unwrap_or(0);
    println!("共找到 {} 条结果:\n", total);

    for (i, r) in results.as_array().unwrap_or(&vec![]).iter().enumerate() {
        println!("  [{}.] {}", i + 1, r["title"].as_str().unwrap_or("N/A"));
        println!("       URL: {}", r["url"].as_str().unwrap_or("N/A"));
        println!("       摘要: {}", r["snippet"].as_str().unwrap_or("N/A"));
        println!();
    }

    // 打印原始 JSON（调试用）
    println!("--- 原始搜索结果 JSON ---");
    println!("{}", serde_json::to_string_pretty(&json).unwrap());
    println!("---\n");

    if total == 0 {
        println!("没有结果，跳过 web_fetch。");
        std::process::exit(0);
    }

    // ==================== 第二步: web_fetch ====================
    let idx = if result_idx < total { result_idx } else { 0 };
    let target = &results[idx];
    let url = target["url"].as_str().unwrap_or("").to_string();

    println!();
    println!("╔══════════════════════════════════════════════╗");
    println!("║           第二步: Web Fetch                  ║");
    println!("╚══════════════════════════════════════════════╝");
    println!("抓取第 {} 条结果: {}", idx + 1, url);
    println!();

    let raw_fetch = WebFetch.call(url, None, None, None).await;
    let fetch_json: serde_json::Value =
        serde_json::from_str(&raw_fetch).expect("解析 web_fetch 返回的 JSON 失败");

    if fetch_json["ok"] == false {
        eprintln!("抓取失败!");
        eprintln!("错误: {}", fetch_json["error"].as_str().unwrap_or("未知错误"));
        if let Some(suggestion) = fetch_json["suggestion"].as_str() {
            eprintln!("建议: {}", suggestion);
        }
        std::process::exit(1);
    }

    let data = &fetch_json["data"];
    println!("标题: {}", data["title"].as_str().unwrap_or("N/A"));
    println!("URL: {}", data["url"].as_str().unwrap_or("N/A"));
    if let Some(final_url) = data["final_url"].as_str() {
        if !final_url.is_empty() && final_url != data["url"].as_str().unwrap_or("") {
            println!("最终 URL: {}", final_url);
        }
    }
    println!(
        "内容长度: {} 字节",
        data["content_length"].as_u64().unwrap_or(0)
    );
    println!(
        "已截断: {}",
        if data["truncated"].as_bool().unwrap_or(false) {
            "是"
        } else {
            "否"
        }
    );
    println!();

    println!("--- 页面内容 ---");
    println!("{}", data["content"].as_str().unwrap_or("(空)"));
}
