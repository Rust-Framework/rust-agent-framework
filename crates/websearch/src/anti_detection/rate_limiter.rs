//! 速率限制器 —— 确保请求间隔 + 随机抖动，避免触发反爬限流。

use rand::Rng;
use std::sync::Mutex;
use std::time::Instant;
use tokio::time::sleep;

/// 请求速率限制器。
///
/// 线程安全，可在多个并发任务间共享。
#[derive(Debug)]
pub struct RateLimiter {
    last_request: Mutex<Option<Instant>>,
}

impl RateLimiter {
    /// 创建新的速率限制器。
    pub fn new() -> Self {
        Self {
            last_request: Mutex::new(None),
        }
    }

    /// 等待足够的时间以遵守最小请求间隔。
    ///
    /// 实际等待 = `min_interval_ms + random_jitter`，
    /// 其中 jitter 为 ±30% 的随机偏移。
    ///
    /// 注意：锁在 `await` 之前释放，确保 future 是 `Send` 的。
    pub async fn wait(&self, min_interval_ms: u64) {
        let maybe_delay = {
            let mut last = self.last_request.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();

            if let Some(last_time) = *last {
                let elapsed = now.duration_since(last_time);
                let min_duration = std::time::Duration::from_millis(min_interval_ms);

                if elapsed < min_duration {
                    let remaining = min_duration - elapsed;
                    let jitter = {
                        let mut rng = rand::thread_rng();
                        let jitter_range = (min_interval_ms as f64 * 0.3) as i64;
                        let jitter_ms = rng.gen_range(-jitter_range..=jitter_range);
                        let jittered = remaining.as_millis() as i64 + jitter_ms;
                        std::time::Duration::from_millis(jittered.max(0) as u64)
                    };
                    Some(jitter)
                } else {
                    *last = Some(now);
                    None
                }
            } else {
                *last = Some(now);
                None
            }
        }; // 锁在此处释放

        if let Some(delay) = maybe_delay {
            sleep(delay).await;
            let mut last = self.last_request.lock().unwrap_or_else(|e| e.into_inner());
            *last = Some(Instant::now());
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let limiter = RateLimiter::new();
        let start = Instant::now();

        limiter.wait(100).await;
        limiter.wait(100).await;

        let elapsed = start.elapsed();
        // 第一次等待立即通过（无历史），第二次约 100ms ±30% 抖动
        assert!(elapsed.as_millis() >= 50, "elapsed: {}ms", elapsed.as_millis());
    }
}
