//! Single-threaded curator worker with latest-wins job coalescing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use rust_agent_core::{ChatMessage, IChatClient};
use tokio::sync::mpsc;

use super::agent::run_curator;
use super::trace::{
    emit_worker_coalesced, emit_worker_finished, emit_worker_started, ConsolidationRunContext,
};

/// A single consolidation job in the worker queue.
#[derive(Clone)]
pub struct ConsolidationJob {
    pub bundle_dir: PathBuf,
    pub client: Arc<dyn IChatClient>,
    pub messages: Vec<ChatMessage>,
    pub session_id: Option<String>,
    pub coalesced_dropped: u64,
}

/// Worker statistics exposed for debugging.
#[derive(Debug, Clone, Default)]
pub struct WorkerStats {
    pub running: bool,
    pub pending: bool,
    pub total_coalesced_dropped: u64,
    pub total_runs: u64,
}

/// Background consolidation worker — serial execution, latest-wins coalescing.
pub struct ConsolidationWorker {
    tx: mpsc::UnboundedSender<ConsolidationJob>,
    running: Arc<AtomicBool>,
    queued: Arc<AtomicUsize>,
    total_coalesced_dropped: Arc<AtomicU64>,
    total_runs: Arc<AtomicU64>,
}

impl ConsolidationWorker {
    pub fn spawn() -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<ConsolidationJob>();
        let running = Arc::new(AtomicBool::new(false));
        let queued = Arc::new(AtomicUsize::new(0));
        let total_coalesced_dropped = Arc::new(AtomicU64::new(0));
        let total_runs = Arc::new(AtomicU64::new(0));

        let worker = Arc::new(Self {
            tx,
            running: Arc::clone(&running),
            queued: Arc::clone(&queued),
            total_coalesced_dropped: Arc::clone(&total_coalesced_dropped),
            total_runs: Arc::clone(&total_runs),
        });

        tokio::spawn(async move {
            while let Some(first) = rx.recv().await {
                let (mut job, dropped) = coalesce_after_recv(&mut rx, first);
                queued.fetch_sub(1 + dropped as usize, Ordering::SeqCst);

                if dropped > 0 {
                    emit_worker_coalesced(dropped);
                    total_coalesced_dropped.fetch_add(dropped, Ordering::Relaxed);
                    job.coalesced_dropped += dropped;
                }

                let ctx = ConsolidationRunContext::new(
                    job.session_id.clone(),
                    job.messages.len(),
                    job.coalesced_dropped,
                );

                running.store(true, Ordering::SeqCst);
                emit_worker_started(&ctx);

                let status = run_curator(
                    job.bundle_dir,
                    job.client,
                    job.messages,
                    ctx.clone(),
                )
                .await;

                emit_worker_finished(&ctx, status.as_str());
                running.store(false, Ordering::SeqCst);
                total_runs.fetch_add(1, Ordering::Relaxed);
            }
        });

        worker
    }

    pub fn enqueue_latest(&self, job: ConsolidationJob) {
        self.queued.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(job);
    }

    pub fn stats(&self) -> WorkerStats {
        WorkerStats {
            running: self.running.load(Ordering::SeqCst),
            pending: self.queued.load(Ordering::SeqCst) > 0,
            total_coalesced_dropped: self.total_coalesced_dropped.load(Ordering::Relaxed),
            total_runs: self.total_runs.load(Ordering::Relaxed),
        }
    }
}

pub(crate) fn coalesce_after_recv<T>(
    rx: &mut mpsc::UnboundedReceiver<T>,
    mut job: T,
) -> (T, u64) {
    let mut dropped = 0u64;
    while let Ok(next) = rx.try_recv() {
        job = next;
        dropped += 1;
    }
    (job, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn coalesce_drain_keeps_latest() {
        let (tx, mut rx) = mpsc::unbounded_channel::<usize>();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();

        let first = rx.recv().await.unwrap();
        let (job, dropped) = coalesce_after_recv(&mut rx, first);
        assert_eq!(job, 3);
        assert_eq!(dropped, 2);
    }

    #[tokio::test]
    async fn running_job_then_pending_coalesces_to_latest() {
        let (tx, mut rx) = mpsc::unbounded_channel::<usize>();
        tx.send(2).unwrap();
        tx.send(3).unwrap();

        let first = rx.recv().await.unwrap();
        let (job, dropped) = coalesce_after_recv(&mut rx, first);
        assert_eq!(job, 3);
        assert_eq!(dropped, 1);
    }

    #[tokio::test]
    async fn full_sequence_executes_job1_then_job3_only() {
        let mut executions = Vec::new();
        executions.push(1);

        let (tx, mut rx) = mpsc::unbounded_channel::<usize>();
        tx.send(2).unwrap();
        tx.send(3).unwrap();

        let first = rx.recv().await.unwrap();
        let (job, dropped) = coalesce_after_recv(&mut rx, first);
        executions.push(job);

        assert_eq!(dropped, 1);
        assert_eq!(executions, vec![1, 3]);
    }
}
