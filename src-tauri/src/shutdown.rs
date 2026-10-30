//! Cancellation never competes for space in the session's command queue.
use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::watch;

#[derive(Clone)]
pub struct Shutdown(Arc<Inner>);
struct Inner {
    started: AtomicBool,
    finished: AtomicBool,
    signal: watch::Sender<bool>,
}
impl Default for Shutdown {
    fn default() -> Self {
        Self(Arc::new(Inner {
            started: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            signal: watch::channel(false).0,
        }))
    }
}
impl Shutdown {
    pub fn begin(&self) -> bool {
        if self.0.started.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.0.signal.send_replace(true);
        true
    }
    pub fn finished(&self) -> bool {
        self.0.finished.load(Ordering::SeqCst)
    }
    pub fn finish(&self) {
        self.0.finished.store(true, Ordering::SeqCst);
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.0.signal.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
    pub async fn run<T>(
        &self,
        future: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        tokio::select! {
            biased;
            _ = self.cancelled() => Err("The app is closing".into()),
            result = future => result,
        }
    }
}

/// The actor owns the browser. Aborting it drops the child handle, whose
/// kill-on-drop policy is the final fallback if graceful browser close stalls.
pub async fn finish_session(mut task: tokio::task::JoinHandle<()>) {
    match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
        Ok(Ok(())) => return,
        Ok(Err(error)) => {
            tracing::warn!(%error, "session stopped during shutdown");
            return;
        }
        Err(_) => tracing::warn!("graceful shutdown exceeded three seconds; aborting session"),
    }
    task.abort();
    if tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .is_err()
    {
        tracing::warn!("session did not stop after cancellation; exiting");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancellation_bypasses_a_full_queue_and_rejects_new_work() {
        let shutdown = Shutdown::default();
        let (sender, _receiver) = tokio::sync::mpsc::channel(1);
        sender.send(1).await.unwrap();
        let stop = shutdown.clone();
        let blocked = tokio::spawn(async move {
            stop.run(async { sender.send(2).await.map_err(|e| e.to_string()) })
                .await
        });
        tokio::task::yield_now().await;
        assert!(shutdown.begin());
        assert!(!shutdown.begin());
        assert!(blocked.await.unwrap().is_err());
        assert!(shutdown.run(async { Ok(()) }).await.is_err());
    }
    #[tokio::test(start_paused = true)]
    async fn stalled_work_is_dropped_after_the_grace_period() {
        struct Resource(Arc<AtomicBool>);
        impl Drop for Resource {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let resource = Resource(dropped.clone());
        let task = tokio::spawn(async move {
            let _resource = resource;
            std::future::pending::<()>().await;
        });
        let start = tokio::time::Instant::now();
        finish_session(task).await;
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(start.elapsed(), Duration::from_secs(3));
    }
    #[tokio::test(start_paused = true)]
    async fn idle_sessions_exit_without_waiting_for_the_deadline() {
        let start = tokio::time::Instant::now();
        finish_session(tokio::spawn(async {})).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }
}
