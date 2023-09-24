use std::sync::{Arc, Mutex};

use crate::StopReason;

struct Inner {
    finished_callback: Option<Box<dyn FnOnce(StopReason) + Send>>,
    cancelled_callback: Option<Box<dyn FnOnce() + Send>>,
    stop_reason: Option<StopReason>,
}

#[derive(Clone)]
pub(crate) struct AliveGuard {
    inner: Arc<Mutex<Inner>>,
}

impl AliveGuard {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                finished_callback: None,
                cancelled_callback: None,
                stop_reason: None,
            })),
        }
    }

    pub(crate) fn subsystem_ended(&self, reason: StopReason) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(previous_reason) = inner.stop_reason.replace(reason) {
            panic!("This should never happen. Please report this; it indicates a programming error. ({:?})", previous_reason);
        };
    }

    pub(crate) fn on_cancel(&self, cancelled_callback: impl FnOnce() + 'static + Send) {
        let mut inner = self.inner.lock().unwrap();
        assert!(inner.cancelled_callback.is_none());
        inner.cancelled_callback = Some(Box::new(cancelled_callback));
    }

    pub(crate) fn on_finished(&self, finished_callback: impl FnOnce(StopReason) + 'static + Send) {
        let mut inner = self.inner.lock().unwrap();
        assert!(inner.finished_callback.is_none());
        inner.finished_callback = Some(Box::new(finished_callback));
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let finished_callback = self
            .finished_callback
            .take()
            .expect("No `finished` callback was registered in AliveGuard!");

        finished_callback(self.stop_reason.take().unwrap_or(StopReason::Cancelled));

        if let Some(cancelled_callback) = self.cancelled_callback.take() {
            cancelled_callback()
        }
    }
}

#[cfg(test)]
mod tests {

    use std::sync::atomic::{AtomicU32, Ordering};

    use tokio::time::{timeout, Duration};

    use super::*;
    use crate::utils::JoinerToken;

    #[test]
    fn fallback_is_cancelled() {
        let alive_guard = AliveGuard::new();

        let (set_result, mut result) = tokio::sync::oneshot::channel();

        alive_guard.on_finished(move |stopreason| {
            set_result.send(stopreason).unwrap();
        });

        drop(alive_guard);

        assert!(matches!(result.try_recv().unwrap(), StopReason::Cancelled));
    }

    #[test]
    fn normal_operation() {
        let alive_guard = AliveGuard::new();

        let (set_result, mut result) = tokio::sync::oneshot::channel();

        alive_guard.on_finished(move |stopreason| {
            set_result.send(stopreason).unwrap();
        });

        alive_guard.subsystem_ended(StopReason::Panic);

        drop(alive_guard);

        assert!(matches!(result.try_recv().unwrap(), StopReason::Panic));
    }

    #[test]
    fn inverted_setup_order() {
        let alive_guard = AliveGuard::new();

        let (set_result, mut result) = tokio::sync::oneshot::channel();

        alive_guard.subsystem_ended(StopReason::Panic);

        alive_guard.on_finished(move |stopreason| {
            set_result.send(stopreason).unwrap();
        });

        drop(alive_guard);

        assert!(matches!(result.try_recv().unwrap(), StopReason::Panic));
    }

    #[test]
    fn cancel_callback_with_finished() {
        let alive_guard = AliveGuard::new();

        let counter = Arc::new(AtomicU32::new(0));
        let counter2 = Arc::clone(&counter);

        alive_guard.on_finished(|_| {});
        alive_guard.on_cancel(move || {
            counter2.fetch_add(1, Ordering::Relaxed);
        });

        alive_guard.subsystem_ended(StopReason::Panic);

        drop(alive_guard);

        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancel_callback_with_finished_inverted_order() {
        let alive_guard = AliveGuard::new();

        let counter = Arc::new(AtomicU32::new(0));
        let counter2 = Arc::clone(&counter);

        alive_guard.subsystem_ended(StopReason::Panic);

        alive_guard.on_finished(|_| {});
        alive_guard.on_cancel(move || {
            counter2.fetch_add(1, Ordering::Relaxed);
        });

        drop(alive_guard);

        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancel_callback_without_finished() {
        let alive_guard = AliveGuard::new();

        let counter = Arc::new(AtomicU32::new(0));
        let counter2 = Arc::clone(&counter);

        alive_guard.on_finished(|_| {});
        alive_guard.on_cancel(move || {
            counter2.fetch_add(1, Ordering::Relaxed);
        });

        drop(alive_guard);

        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    #[should_panic(expected = "No `finished` callback was registered in AliveGuard!")]
    fn panic_if_no_finished_callback_set() {
        let alive_guard = AliveGuard::new();
        drop(alive_guard);
    }
}
