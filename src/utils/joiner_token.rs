use std::{fmt::Debug, sync::Arc};

use tokio::sync::watch;

struct Inner {
    counter: watch::Sender<u32>,
    parent: Option<Arc<Inner>>,
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
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                counter: watch::channel(0).0,
                parent: None,
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

    pub(crate) fn create_child_token(&self) -> Self {
        let mut maybe_parent = Some(&self.inner);
        while let Some(parent) = maybe_parent {
            parent.counter.send_modify(|val| *val += 1);
            maybe_parent = parent.parent.as_ref();
        }

        Self {
            inner: Arc::new(Inner {
                counter: watch::channel(0).0,
                parent: Some(Arc::clone(&self.inner)),
            }),
        }
    }

    pub(crate) fn count(&self) -> u32 {
        *self.inner.counter.borrow()
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
        let root = JoinerToken::new();
        assert_eq!(0, root.count());

        let child1 = root.create_child_token();
        assert_eq!(1, root.count());
        assert_eq!(0, child1.count());

        let child2 = child1.create_child_token();
        assert_eq!(2, root.count());
        assert_eq!(1, child1.count());
        assert_eq!(0, child2.count());

        let child3 = child1.create_child_token();
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
        let superroot = JoinerToken::new();

        let mut root = superroot.create_child_token();

        let child1 = root.create_child_token();
        let child2 = child1.create_child_token();
        let child3 = child1.create_child_token();

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
        let root = JoinerToken::new();
        assert_eq!(format!("{:?}", root), "JoinerToken(children = 0)");

        let child1 = root.create_child_token();
        assert_eq!(format!("{:?}", root), "JoinerToken(children = 1)");

        let _child2 = child1.create_child_token();
        assert_eq!(format!("{:?}", root), "JoinerToken(children = 2)");
    }
}
