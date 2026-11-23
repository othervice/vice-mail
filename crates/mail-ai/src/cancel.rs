//! Cancelling a request. Dropping a [`crate::ChatStream`] also ends it (the connection closes);
//! the token is for code that does not own the stream, such as a "stop" button.

use std::sync::Arc;

use tokio::sync::watch;

/// Cheap to clone; all clones cancel together. Cancelling is permanent.
#[derive(Debug, Clone)]
pub struct CancelToken {
    state: Arc<watch::Sender<bool>>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            state: Arc::new(watch::channel(false).0),
        }
    }

    pub fn cancel(&self) {
        self.state.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.state.borrow()
    }

    /// Completes once [`CancelToken::cancel`] was called (at once if it was already).
    pub async fn cancelled(&self) {
        let mut receiver = self.state.subscribe();
        // The sender lives in `self`, so the channel cannot close while this waits.
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn clones_cancel_together_and_late_waiters_return_at_once() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        let waiter = tokio::spawn({
            let clone = clone.clone();
            async move { clone.cancelled().await }
        });
        token.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("waiter woke up")
            .expect("task");
        assert!(clone.is_cancelled());
        tokio::time::timeout(std::time::Duration::from_secs(2), clone.cancelled())
            .await
            .expect("already cancelled");
    }
}
