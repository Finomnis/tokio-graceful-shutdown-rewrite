use std::{future::Future, mem::ManuallyDrop, sync::Arc};

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::{
    runner::{AliveGuard, SubsystemRunner},
    utils::{remote_drop_collection::RemotelyDroppableItems, JoinerToken},
    BoxedError, ErrTypeTraits, NestedSubsystem,
};

struct Inner {
    name: Arc<str>,
    cancellation_token: CancellationToken,
    toplevel_cancellation_token: CancellationToken,
    joiner_token: JoinerToken,
    children: RemotelyDroppableItems<SubsystemRunner>,
}

// All the things needed to manage nested subsystems and wait for cancellation
pub struct SubsystemHandle<ErrType: ErrTypeTraits = BoxedError> {
    inner: ManuallyDrop<Inner>,
    // When dropped, redirect Self into this channel.
    // Required as a workaround for https://stackoverflow.com/questions/77172947/async-lifetime-issues-of-pass-by-reference-parameters.
    drop_redirect: Option<oneshot::Sender<SubsystemHandle<ErrType>>>,
}

impl<ErrType: ErrTypeTraits> SubsystemHandle<ErrType> {
    pub fn start<Err, Fut, Subsys>(
        &self,
        name: impl AsRef<str>,
        subsystem: Subsys,
    ) -> NestedSubsystem
    where
        Subsys: 'static + FnOnce(SubsystemHandle<ErrType>) -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), Err>> + Send,
        Err: Into<ErrType>,
    {
        self.start_with_abs_name(
            Arc::from(format!("{}/{}", self.inner.name, name.as_ref())),
            subsystem,
        )
    }

    pub(crate) fn start_with_abs_name<Err, Fut, Subsys>(
        &self,
        name: Arc<str>,
        subsystem: Subsys,
    ) -> NestedSubsystem
    where
        Subsys: 'static + FnOnce(SubsystemHandle<ErrType>) -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), Err>> + Send,
        Err: Into<ErrType>,
    {
        let alive_guard = AliveGuard::new();

        let (joiner_token, joiner_token_ref) = self.inner.joiner_token.child_token(
            |e| Some(e), /* Forward error upwards. TODO: implement handling */
        );
        let cancellation_token = self.inner.cancellation_token.child_token();

        let child_handle = SubsystemHandle {
            inner: ManuallyDrop::new(Inner {
                name: Arc::clone(&name),
                cancellation_token: cancellation_token.clone(),
                toplevel_cancellation_token: self.inner.toplevel_cancellation_token.clone(),
                joiner_token,
                children: RemotelyDroppableItems::new(),
            }),
            drop_redirect: None,
        };

        let runner = SubsystemRunner::new(name, subsystem, child_handle, alive_guard.clone());

        // Shenanigans to juggle child ownership
        //
        // RACE CONDITION SAFETY:
        // If the subsystem ends before `on_finished` was able to be called, nothing bad happens.
        // alive_guard will keep the guard alive and the callback will only be called inside of
        // the guard's drop() implementation.
        let child_dropper = self.inner.children.insert(runner);
        alive_guard.on_finished(|d| drop(child_dropper));

        NestedSubsystem {
            joiner: joiner_token_ref,
            cancellation_token,
        }
    }

    pub async fn wait_for_children(&mut self) {
        self.inner.joiner_token.join_children().await
    }

    // For internal use only - should never be used by users.
    // Required as a short-lived second reference inside of `runner`.
    pub(crate) fn delayed_clone(&mut self) -> oneshot::Receiver<Self> {
        let (sender, receiver) = oneshot::channel();

        let previous = self.drop_redirect.replace(sender);
        assert!(previous.is_none());

        receiver
    }

    pub fn initiate_shutdown(&self) {
        self.inner.toplevel_cancellation_token.cancel();
    }

    pub fn initiate_local_shutdown(&self) {
        self.inner.cancellation_token.cancel();
    }

    pub async fn on_shutdown_requested(&self) {
        self.inner.cancellation_token.cancelled().await
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.inner.cancellation_token.is_cancelled()
    }

    pub(crate) fn get_cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation_token.clone()
    }
}

impl<ErrType: ErrTypeTraits> Drop for SubsystemHandle<ErrType> {
    fn drop(&mut self) {
        // SAFETY: This is how ManuallyDrop is meant to be used.
        // `self.inner` won't ever be used again because `self` will be gone after this
        // function is finished.
        // This takes the `self.inner` object and makes it droppable again.
        //
        // This workaround is required to take ownership for the `self.drop_redirect` channel.
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };

        if let Some(redirect) = self.drop_redirect.take() {
            let redirected_self = Self {
                inner: ManuallyDrop::new(inner),
                drop_redirect: None,
            };

            // ignore error; an error would indicate that there is no receiver.
            // in that case, do nothing.
            let _ = redirect.send(redirected_self);
        }
    }
}

pub(crate) fn root_handle<ErrType: ErrTypeTraits>() -> SubsystemHandle<ErrType> {
    let cancellation_token = CancellationToken::new();

    SubsystemHandle {
        inner: ManuallyDrop::new(Inner {
            name: Arc::from(""),
            toplevel_cancellation_token: cancellation_token.clone(),
            cancellation_token,
            joiner_token: JoinerToken::new(
                |e| panic!("Uncaught error: {:?}", e), /* Panic. TODO: implement proper handling */
            )
            .0,
            children: RemotelyDroppableItems::new(),
        }),
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
        let root_handle = root_handle::<BoxedError>();

        let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

        root_handle.start("", |x| async move {
            drop_sender.send(()).await.unwrap();
            std::future::pending::<Result<(), BoxedError>>().await
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
            std::future::pending::<Result<(), BoxedError>>().await
        };

        let subsys = |x: SubsystemHandle| async move {
            x.start("", subsys2);
            Result::<(), BoxedError>::Ok(())
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
