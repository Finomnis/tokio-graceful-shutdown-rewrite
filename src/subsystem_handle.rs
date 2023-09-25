use std::{future::Future, sync::Arc};

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::{
    runner::{AliveGuard, SubsystemRunner},
    utils::{remote_drop_collection::RemotelyDroppableItems, JoinerToken},
    BoxedError,
};

struct Inner {}

// All the things needed to manage nested subsystems and wait for cancellation
pub struct SubsystemHandle {
    cancellation_token: CancellationToken,
    joiner_token: JoinerToken,
    children: RemotelyDroppableItems<SubsystemRunner>,
    // When dropped, redirect Self into this channel.
    // Required as a workaround for https://stackoverflow.com/questions/77172947/async-lifetime-issues-of-pass-by-reference-parameters.
    drop_redirect: Option<oneshot::Sender<SubsystemHandle>>,
}

impl SubsystemHandle {
    pub fn start<Fut, Subsys>(&self, name: &str, subsystem: Subsys)
    where
        Subsys: 'static + FnOnce(SubsystemHandle) -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
    {
        let alive_guard = AliveGuard::new();

        let child_handle = SubsystemHandle {
            cancellation_token: self.cancellation_token.child_token(),
            joiner_token: self.joiner_token.child_token(
                |e| Some(e), /* Forward error upwards. TODO: implement handling */
            ),
            children: RemotelyDroppableItems::new(),
            drop_redirect: None,
        };

        let runner = SubsystemRunner::new(subsystem, child_handle, alive_guard.clone());

        // Shenanigans to juggle child ownership
        //
        // RACE CONDITION SAFETY:
        // If the subsystem ends before `on_finished` was able to be called, nothing bad happens.
        // alive_guard will keep the guard alive and the callback will only be called inside of
        // the guard's drop() implementation.
        let child_dropper = self.children.insert(runner);
        alive_guard.on_finished(|d| drop(child_dropper));
    }

    pub async fn wait_for_children(&mut self) {
        self.joiner_token.join_children().await
    }

    // For internal use only - should never be used by users.
    // Required as a short-lived second reference inside of `runner`.
    pub(crate) fn delayed_clone(&mut self) -> oneshot::Receiver<SubsystemHandle> {
        let (sender, receiver) = oneshot::channel();

        let previous = self.drop_redirect.replace(sender);
        assert!(previous.is_none());

        receiver
    }
}

impl Drop for SubsystemHandle {
    fn drop(&mut self) {
        if let Some(redirect) = self.drop_redirect.take() {
            let mut handle = root_handle();
            std::mem::swap(&mut handle, self);
            // ignore error; an error would indicate that there is no receiver.
            // in that case, do nothing.
            let _ = redirect.send(handle);
        }
    }
}

pub(crate) fn root_handle() -> SubsystemHandle {
    SubsystemHandle {
        cancellation_token: CancellationToken::new(),
        joiner_token: JoinerToken::new(
            |e| panic!("Uncaught error: {:?}", e), /* Panic. TODO: implement proper handling */
        ),
        children: RemotelyDroppableItems::new(),
        drop_redirect: None,
    }
}

#[cfg(test)]
mod tests {

    use tokio::time::{sleep, timeout, Duration};
    use tracing_test::traced_test;

    use super::*;
    use crate::utils::JoinerToken;

    #[tokio::test]
    async fn recursive_cancellation() {
        let root_handle = root_handle();

        let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

        root_handle.start("", |x| async move {
            drop_sender.send(()).await.unwrap();
            std::future::pending().await
        });

        // Make sure we are executing the subsystem
        let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
            .await
            .unwrap();
        assert!(recv_result.is_some());

        drop(root_handle);

        // Make sure the subsystem got cancelled
        let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
            .await
            .unwrap();
        assert!(recv_result.is_none());
    }

    #[tokio::test]
    async fn recursive_cancellation_2() {
        let root_handle = root_handle();

        let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

        let subsys2 = |_| async move {
            drop_sender.send(()).await.unwrap();
            std::future::pending().await
        };

        let subsys = |x: SubsystemHandle| async move {
            x.start("", subsys2);
            Ok(())
        };

        root_handle.start("", subsys);

        // Make sure we are executing the subsystem
        let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
            .await
            .unwrap();
        assert!(recv_result.is_some());

        // Make sure the grandchild is still running
        sleep(Duration::from_millis(100)).await;
        assert!(matches!(
            drop_receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        drop(root_handle);

        // Make sure the subsystem got cancelled
        let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
            .await
            .unwrap();
        assert!(recv_result.is_none());
    }
}
