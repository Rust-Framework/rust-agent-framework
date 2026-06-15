//! User-Agent 池 —— 随机轮换主流浏览器 User-Agent。
//!
//! 维护 Chrome、Edge、Firefox 最新稳定版的 User-Agent 字符串。

use rand::seq::SliceRandom;

/// 获取一个随机的 User-Agent 字符串。
pub fn random_user_agent() -> &'static str {
    let mut rng = rand::thread_rng();
    USER_AGENTS.choose(&mut rng).copied().unwrap_or(USER_AGENTS[0])
}

/// 主流桌面浏览器 User-Agent 池（2025+ 版本）。
const USER_AGENTS: &[&str] = &[
    // ── Chrome 130+ (Windows) ──
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    // ── Chrome 130+ (macOS) ──
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    // ── Chrome 130+ (Linux) ──
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
    // ── Edge 130+ (Windows) ──
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
    // ── Edge 130+ (macOS) ──
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
    // ── Firefox 132+ (Windows) ──
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:132.0) Gecko/20100101 Firefox/132.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:133.0) Gecko/20100101 Firefox/133.0",
    // ── Firefox 132+ (macOS) ──
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:132.0) Gecko/20100101 Firefox/132.0",
    // ── Firefox 132+ (Linux) ──
    "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_random_user_agent_not_empty() {
        assert!(!random_user_agent().is_empty());
    }

    #[test]
    fn test_user_agents_unique() {
        let set: HashSet<&str> = USER_AGENTS.iter().copied().collect();
        assert_eq!(set.len(), USER_AGENTS.len(), "All user agents should be unique");
    }

    #[test]
    fn test_random_user_agent_in_pool() {
        for _ in 0..50 {
            let ua = random_user_agent();
            assert!(USER_AGENTS.contains(&ua), "UA '{}' not in pool", ua);
        }
    }
}
