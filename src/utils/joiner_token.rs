use std::{fmt::Debug, sync::Arc};

use tokio::sync::watch;

use crate::{BoxedError, StopReason};

struct Inner {
    counter: watch::Sender<u32>,
    parent: Option<Arc<Inner>>,
    on_error: Box<dyn Fn(StopReason) -> Option<StopReason> + Sync + Send>,
}

pub(crate) struct JoinerToken {
    inner: Arc<Inner>,
}

impl Debug for JoinerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JoinerToken(children = {})",
            *self.inner.counter.borrow()
        )
    }
}

impl JoinerToken {
    /// Creates a new joiner token.
    ///
    /// The `on_error` callback will receive errors/panics and has to decide
    /// how to handle them. It can also not handle them and instead pass them on.
    /// If it returns `Some`, the error will get passed on to its parent.
    pub(crate) fn new(
        on_error: impl Fn(StopReason) -> Option<StopReason> + Sync + Send + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                counter: watch::channel(0).0,
                parent: None,
                on_error: Box::new(on_error),
            }),
        }
    }

    // Requires `mut` access to prevent children from being spawned
    // while waiting
    pub(crate) async fn join_children(&mut self) {
        let mut subscriber = self.inner.counter.subscribe();

        // Ignore errors; if the channel got closed, that definitely means
        // no more children exist.
        let _ = subscriber.wait_for(|val| *val == 0).await;
    }

    pub(crate) fn create_child_token(
        &self,
        on_error: impl Fn(StopReason) -> Option<StopReason> + Sync + Send + 'static,
    ) -> Self {
        let mut maybe_parent = Some(&self.inner);
        while let Some(parent) = maybe_parent {
            parent.counter.send_modify(|val| *val += 1);
            maybe_parent = parent.parent.as_ref();
        }

        Self {
            inner: Arc::new(Inner {
                counter: watch::channel(0).0,
                parent: Some(Arc::clone(&self.inner)),
                on_error: Box::new(on_error),
            }),
        }
    }

    pub(crate) fn count(&self) -> u32 {
        *self.inner.counter.borrow()
    }

    pub(crate) fn raise_failure(&self, mut stop_reason: StopReason) {
        let mut maybe_stop_reason = Some(stop_reason);

        let mut maybe_parent = self.inner.parent.as_ref();
        while let Some(parent) = maybe_parent {
            if let Some(stop_reason) = maybe_stop_reason {
                maybe_stop_reason = (parent.on_error)(stop_reason);
            } else {
                break;
            }

            maybe_parent = parent.parent.as_ref();
        }

        if let Some(stop_reason) = maybe_stop_reason {
            tracing::warn!("Unhandled stop reason: {:?}", stop_reason);
        }
    }
}

impl Drop for JoinerToken {
    fn drop(&mut self) {
        let mut maybe_parent = self.inner.parent.as_ref();
        while let Some(parent) = maybe_parent {
            parent.counter.send_modify(|val| *val -= 1);
            maybe_parent = parent.parent.as_ref();
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::{sleep, timeout, Duration};

    use super::*;

    #[test]
    fn counters() {
        let root = JoinerToken::new(|_| None);
        assert_eq!(0, root.count());

        let child1 = root.create_child_token(|_| None);
        assert_eq!(1, root.count());
        assert_eq!(0, child1.count());

        let child2 = child1.create_child_token(|_| None);
        assert_eq!(2, root.count());
        assert_eq!(1, child1.count());
        assert_eq!(0, child2.count());

        let child3 = child1.create_child_token(|_| None);
        assert_eq!(3, root.count());
        assert_eq!(2, child1.count());
        assert_eq!(0, child2.count());
        assert_eq!(0, child3.count());

        drop(child1);
        assert_eq!(2, root.count());
        assert_eq!(0, child2.count());
        assert_eq!(0, child3.count());

        drop(child2);
        assert_eq!(1, root.count());
        assert_eq!(0, child3.count());

        drop(child3);
        assert_eq!(0, root.count());
    }

    #[tokio::test]
    async fn join() {
        let superroot = JoinerToken::new(|_| None);

        let mut root = superroot.create_child_token(|_| None);

        let child1 = root.create_child_token(|_| None);
        let child2 = child1.create_child_token(|_| None);
        let child3 = child1.create_child_token(|_| None);

        let (set_finished, mut finished) = tokio::sync::oneshot::channel();
        tokio::join!(
            async {
                timeout(Duration::from_millis(500), root.join_children())
                    .await
                    .unwrap();
                set_finished.send(root.count()).unwrap();
            },
            async {
                sleep(Duration::from_millis(50)).await;
                assert!(finished.try_recv().is_err());

                drop(child1);
                sleep(Duration::from_millis(50)).await;
                assert!(finished.try_recv().is_err());

                drop(child2);
                sleep(Duration::from_millis(50)).await;
                assert!(finished.try_recv().is_err());

                drop(child3);
                sleep(Duration::from_millis(50)).await;
                let count = timeout(Duration::from_millis(50), finished)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(count, 0);
            }
        );
    }

    #[test]
    fn debug_print() {
        let root = JoinerToken::new(|_| None);
        assert_eq!(format!("{:?}", root), "JoinerToken(children = 0)");

        let child1 = root.create_child_token(|_| None);
        assert_eq!(format!("{:?}", root), "JoinerToken(children = 1)");

        let _child2 = child1.create_child_token(|_| None);
        assert_eq!(format!("{:?}", root), "JoinerToken(children = 2)");
    }
}
