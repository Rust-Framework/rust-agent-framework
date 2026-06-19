//! servo-fetch-worker — 隔离 Servo 崩溃的子进程 worker。
//!
//! 将 servo-fetch 运行在独立子进程中。当 Servo 内部线程（如 StyleThread）
//！发生栈溢出时，仅子进程崩溃，父进程不受影响并可回退到 scraper。
//!
//! ## 协议
//!
//! 参数：`<url> <timeout_secs> <settle_ms> <user_agent>`
//!
//! 成功时向 stdout 输出一行 JSON：`{"ok":true,"title":"...","content":"..."}`
//! 失败时向 stderr 输出错误信息，退出码非 0。
//!
//! 退出码：
//! - 0  — 成功
//! - 1  — servo-fetch 错误（超时、网络错误等）
//! - 2  — 参数错误
//! - 其他 — Servo 崩溃（栈溢出等），由 OS 信号/异常码决定

use std::io::Write;
use std::time::Duration;

fn main() {
    // 抑制 panic 默认输出——崩溃信息走 stderr，保持 stdout 干净
    std::panic::set_hook(Box::new(|info| {
        let _ = writeln!(std::io::stderr(), "worker panic: {info}");
    }));

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: servo-fetch-worker <url> [timeout_secs] [settle_ms] [user_agent]");
        std::process::exit(2);
    }

    let url = &args[1];
    let timeout_secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);
    let settle_ms: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let user_agent: Option<&str> = args.get(4).filter(|s| !s.is_empty()).map(|s| s.as_str());

    // 使用 current-thread runtime，减少资源占用
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to build tokio runtime: {e}");
            std::process::exit(1);
        }
    };

    let result = runtime.block_on(fetch(url, timeout_secs, settle_ms, user_agent));

    match result {
        Ok((title, content)) => {
            let json = serde_json::json!({
                "ok": true,
                "title": title,
                "content": content,
            });
            // 输出到 stdout——父进程读取此行
            let serialized = serde_json::to_string(&json).unwrap_or_else(|_| r#"{"ok":false}"#.to_string());
            println!("{serialized}");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("servo-fetch error: {e}");
            std::process::exit(1);
        }
    }
}

async fn fetch(
    url: &str,
    timeout_secs: u64,
    settle_ms: u64,
    user_agent: Option<&str>,
) -> Result<(String, String), servo_fetch::Error> {
    let mut opts = servo_fetch::FetchOptions::new(url)
        .timeout(Duration::from_secs(timeout_secs));

    if settle_ms > 0 {
        opts = opts.settle(Duration::from_millis(settle_ms));
    }

    if let Some(ua) = user_agent {
        opts = opts.user_agent(ua);
    }

    let page = servo_fetch::fetch(&opts).await?;

    let content = page.markdown().unwrap_or_else(|_| page.inner_text.clone());
    let title = page.title.clone().unwrap_or_default();

    Ok((title, content))
}
