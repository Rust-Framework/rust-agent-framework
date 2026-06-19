//! 网页内容抓取。
//!
//! 基于 servo-fetch（内嵌 Servo 浏览器引擎）实现浏览器级网页渲染和内容提取。
//! 支持 JavaScript 执行、布局感知正文提取、SPA 页面水合等待。
//!
//! ## 崩溃隔离
//!
//! servo-fetch 运行在独立子进程（`servo-fetch-worker`）中。Servo 内部的
//! StyleThread 栈溢出（`STATUS_STACK_OVERFLOW`）是 OS 级异常，无法被
//! `catch_unwind` 捕获。通过子进程隔离，Servo 崩溃时仅 worker 退出，
//! 父进程不受影响并自动回退到 scraper。
//!
//! ## 内容提取管线
//!
//! 1. servo-fetch worker（子进程）渲染并提取 Markdown
//! 2. ContentCleaner 后处理清洗（去噪、页脚检测）
//! 3. 质量评分——若不达标，回退到 scraper 提取
//! 4. 截断保护

use crate::anti_detection::RateLimiter;
use crate::content_cleaner::{ContentCleaner, score_content};
use crate::error::SearchError;
use crate::types::{FetchConfig, FetchedPage};
use std::sync::Arc;
use std::time::Duration;

fn fetch_rate_limiter() -> &'static Arc<RateLimiter> {
    static LIMITER: std::sync::OnceLock<Arc<RateLimiter>> = std::sync::OnceLock::new();
    LIMITER.get_or_init(|| Arc::new(RateLimiter::new()))
}

/// 抓取网页内容。
///
/// ## 处理流程
///
/// 1. 速率控制
/// 2. 使用 servo-fetch worker（子进程）渲染页面（含 JS 执行）
/// 3. 提取可读 Markdown 内容（布局感知，自动去除导航/页脚/广告）
/// 4. ContentCleaner 后处理清洗
/// 5. 质量评分——若不达标且回退启用，使用 scraper 重试
/// 6. 内容截断保护
pub async fn fetch_page(url: &str, config: &FetchConfig) -> Result<FetchedPage, SearchError> {
    if url.is_empty() {
        return Err(SearchError::Config("URL cannot be empty".into()));
    }

    // 速率控制
    fetch_rate_limiter().wait(config.min_interval_ms).await;

    let cleaner = ContentCleaner::new(config.clean_mode);

    // 尝试 servo-fetch（子进程隔离）
    let servo_result = if config.use_servo && !is_servo_unsafe_domain(url) {
        try_servo_fetch_subprocess(url, config).await
    } else {
        if !config.use_servo {
            tracing::debug!(url = %url, "servo-fetch disabled by config, using scraper directly");
        }
        Err(SearchError::Other("servo-fetch skipped".into()))
    };

    let (title, raw_content, final_url) = match servo_result {
        Ok((t, c, fu)) => (t, c, fu),
        Err(e) => {
            tracing::warn!(url = %url, error = %e, "servo-fetch failed, attempting scraper fallback");

            if config.fallback_enabled {
                return crate::scraper_fallback::extract_with_scraper(url, config).await;
            }
            return Err(e);
        }
    };

    // 后处理清洗
    let cleaned = cleaner.clean(&raw_content);

    tracing::debug!(
        url = %url,
        raw_len = raw_content.len(),
        cleaned_len = cleaned.len(),
        "Content cleaned"
    );

    // 质量评分
    let quality = score_content(&cleaned);
    tracing::debug!(url = %url, quality = quality, threshold = config.quality_threshold, "Content quality scored");

    // 决定最终使用的 content
    let (content, source) = if quality < config.quality_threshold && config.fallback_enabled {
        tracing::info!(
            url = %url,
            quality = quality,
            threshold = config.quality_threshold,
            "Quality below threshold, attempting scraper fallback"
        );

        match crate::scraper_fallback::extract_with_scraper(url, config).await {
            Ok(fallback_page) => {
                let fallback_cleaned = cleaner.clean(&fallback_page.content);
                let fallback_quality = score_content(&fallback_cleaned);

                if fallback_quality > quality {
                    tracing::info!(
                        url = %url,
                        fallback_quality = fallback_quality,
                        original_quality = quality,
                        "Scraper fallback produced better content"
                    );
                    (fallback_cleaned, "scraper-fallback")
                } else {
                    tracing::info!(
                        url = %url,
                        "Scraper fallback did not improve quality, keeping servo-fetch output"
                    );
                    (cleaned, "servo-fetch")
                }
            }
            Err(e) => {
                tracing::warn!(url = %url, error = %e, "Scraper fallback failed, keeping servo-fetch output");
                (cleaned, "servo-fetch")
            }
        }
    } else {
        (cleaned, "servo-fetch")
    };

    tracing::info!(
        url = %url,
        title = %title,
        content_len = content.len(),
        quality = quality,
        source = source,
        "Page fetched successfully"
    );

    // 截断处理
    let content_length = content.len();
    let (content, truncated) = if content_length > config.max_content_bytes {
        let truncate_at = content
            .char_indices()
            .take(config.max_content_bytes)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        let truncated_content = &content[..truncate_at];
        let truncated = format!(
            "{truncated_content}\n\n[Content truncated: {total} bytes total, showing first {shown} bytes. Use a smaller scope or more specific query to get relevant data.]",
            total = content_length,
            shown = truncate_at
        );
        (truncated, true)
    } else {
        (content, false)
    };

    Ok(FetchedPage {
        url: url.to_string(),
        final_url,
        title,
        content,
        content_length,
        truncated,
        status_code: 200,
    })
}

// ── 子进程隔离的 servo-fetch ──

/// 已知会触发 Servo 栈溢出的域名。
///
/// 这些站点 CSS 复杂度极高，会导致 Servo StyleThread 递归过深而栈溢出。
/// 列入此表的域名将直接使用 scraper，跳过 servo-fetch。
fn is_servo_unsafe_domain(url: &str) -> bool {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default();

    const UNSAFE_DOMAINS: &[&str] = &[
        "cnblogs.com",
        "www.cnblogs.com",
    ];

    UNSAFE_DOMAINS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// 定位 servo-fetch-worker 二进制文件。
///
/// 搜索顺序：
/// 1. `SERVO_FETCH_WORKER_PATH` 环境变量
/// 2. 当前可执行文件同目录下的 `servo-fetch-worker` / `.exe`
/// 3. PATH 中的 `servo-fetch-worker`
fn find_worker_binary() -> Option<std::path::PathBuf> {
    // 1. 环境变量
    if let Ok(path) = std::env::var("SERVO_FETCH_WORKER_PATH") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. 当前可执行文件同目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                dir.join("servo-fetch-worker.exe"),
                dir.join("servo-fetch-worker"),
            ];
            for c in &candidates {
                if c.exists() {
                    return Some(c.clone());
                }
            }
        }
    }

    // 3. 从 PATH 查找（which/where）
    which_worker()
}

#[cfg(windows)]
fn which_worker() -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("where")
        .arg("servo-fetch-worker.exe")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    let p = std::path::PathBuf::from(path);
    if p.exists() { Some(p) } else { None }
}

#[cfg(not(windows))]
fn which_worker() -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("which")
        .arg("servo-fetch-worker")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let p = std::path::PathBuf::from(path);
    if p.exists() { Some(p) } else { None }
}

/// 通过子进程调用 servo-fetch worker，隔离 Servo 崩溃。
///
/// 如果 worker 崩溃（栈溢出等 OS 级异常），返回错误以触发 scraper 回退。
async fn try_servo_fetch_subprocess(
    url: &str,
    config: &FetchConfig,
) -> Result<(String, String, String), SearchError> {
    let worker_bin = find_worker_binary().ok_or_else(|| {
        SearchError::Other(
            "servo-fetch-worker binary not found; set SERVO_FETCH_WORKER_PATH or build the websearch crate's bin target".into(),
        )
    })?;

    let ua = config
        .user_agent
        .as_deref()
        .unwrap_or_else(|| crate::anti_detection::random_user_agent());

    tracing::debug!(
        url = %url,
        worker = %worker_bin.display(),
        timeout_secs = config.timeout_secs,
        settle_ms = config.settle_ms,
        "Spawning servo-fetch worker subprocess"
    );

    let mut cmd = tokio::process::Command::new(&worker_bin);
    cmd.arg(url)
        .arg(config.timeout_secs.to_string())
        .arg(config.settle_ms.to_string())
        .arg(ua)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // 隐藏 Windows 控制台窗口
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // 子进程总超时 = 页面超时 + 10s 余量（worker 启动 + 关闭）
    let subprocess_timeout = Duration::from_secs(config.timeout_secs + 10);

    let child = cmd.spawn().map_err(|e| {
        SearchError::Network(format!("failed to spawn servo-fetch worker: {e}"))
    })?;

    let wait_future = async {
        let output = child.wait_with_output().await.map_err(|e| {
            SearchError::Network(format!("servo-fetch worker wait failed: {e}"))
        })?;
        Ok::<_, SearchError>(output)
    };

    let output = match tokio::time::timeout(subprocess_timeout, wait_future).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            tracing::warn!(url = %url, "servo-fetch worker timed out, falling back to scraper");
            return Err(SearchError::Timeout(format!(
                "servo-fetch worker subprocess timed out for {url}"
            )));
        }
    };

    let exit_status = output.status;

    // 检查是否崩溃——非正常退出码
    if !exit_status.success() {
        let code = exit_status.code();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_trim = stderr.trim();

        // Windows STATUS_STACK_OVERFLOW = 0xC00000FD
        // ExitStatus::code() 返回 i32，0xC00000FD 超出 i32 范围，
        // 需转为 u32 比较
        let exit_code_u32 = code.map(|c| c as u32).unwrap_or(0);
        let is_stack_overflow = exit_code_u32 == 0xC00000FD
            || stderr_trim.contains("stack overflow")
            || stderr_trim.contains("overflowed its stack");

        if is_stack_overflow {
            tracing::warn!(
                url = %url,
                exit_code = ?code,
                "servo-fetch worker crashed with stack overflow — consider adding domain to unsafe list"
            );
            return Err(SearchError::Network(format!(
                "servo-fetch worker crashed (stack overflow) for {url}"
            )));
        }

        // 普通错误（超时、网络错误等）
        return Err(SearchError::Network(format!(
            "servo-fetch worker exited with code {code:?}: {stderr_trim}"
        )));
    }

    // 解析 stdout JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout_trim = stdout.trim();

    if stdout_trim.is_empty() {
        return Err(SearchError::Parse(
            "servo-fetch worker produced empty output".into(),
        ));
    }

    let json: serde_json::Value = serde_json::from_str(stdout_trim).map_err(|e| {
        SearchError::Parse(format!("failed to parse worker output as JSON: {e}"))
    })?;

    let ok = json.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        let err_msg = json
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown worker error");
        return Err(SearchError::Other(format!("servo-fetch worker error: {err_msg}")));
    }

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let content = json
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let final_url = url.to_string(); // servo-fetch 目前不暴露 final URL

    Ok((title, content, final_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_config_default() {
        let config = FetchConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_content_bytes, 50 * 1024);
        assert!(config.user_agent.is_none());
        assert_eq!(config.settle_ms, 0);
        assert!(config.fallback_enabled);
        assert_eq!(config.quality_threshold, 0.4);
        assert!(config.use_servo);
    }

    #[test]
    fn test_is_servo_unsafe_domain_cnblogs() {
        assert!(is_servo_unsafe_domain("https://www.cnblogs.com/wintersun/p/19145808"));
        assert!(is_servo_unsafe_domain("https://cnblogs.com/someuser/p/123"));
    }

    #[test]
    fn test_is_servo_unsafe_domain_safe() {
        assert!(!is_servo_unsafe_domain("https://example.com"));
        assert!(!is_servo_unsafe_domain("https://github.com/microsoft/agent-framework"));
        assert!(!is_servo_unsafe_domain("https://learn.microsoft.com/en-us/agent-framework"));
    }

    #[test]
    fn test_is_servo_unsafe_domain_invalid_url() {
        // 无效 URL 不应 panic
        assert!(!is_servo_unsafe_domain("not a url"));
        assert!(!is_servo_unsafe_domain(""));
    }
}
