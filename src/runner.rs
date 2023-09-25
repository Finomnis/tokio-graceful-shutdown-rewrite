//! The SubsystemRunner is a little tricky, so here some explanation.
//!
//! A two-layer `tokio::spawn` is required to make this work reliably; the inner `spawn` is the actual subsystem,
//! and the outer `spawn` carries out the duty of propagating the `StopReason`.
//!
//! Further, everything in here reacts properly to being dropped, including the `AliveGuard` (propagating `StopReason::Cancel` in that case)
//! and runner itself, who cancels the subsystem on drop.

use std::{future::Future, sync::Arc};

use crate::{BoxedError, StopReason, SubsystemHandle};

mod alive_guard;
pub(crate) use self::alive_guard::AliveGuard;

pub(crate) struct SubsystemRunner {
    aborthandle: tokio::task::AbortHandle,
}

impl SubsystemRunner {
    pub(crate) fn new<Fut, Subsys>(
        subsystem: Subsys,
        subsystem_handle: SubsystemHandle,
        mut guard: AliveGuard,
    ) -> Self
    where
        Subsys: 'static + FnOnce(SubsystemHandle) -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
    {
        let aborthandle =
            tokio::spawn(run_subsystem(subsystem, subsystem_handle, guard)).abort_handle();
        SubsystemRunner { aborthandle }
    }
}

impl Drop for SubsystemRunner {
    fn drop(&mut self) {
        self.aborthandle.abort()
    }
}

async fn run_subsystem<Fut, Subsys>(
    subsystem: Subsys,
    mut subsystem_handle: SubsystemHandle,
    mut guard: AliveGuard,
) where
    Subsys: 'static + FnOnce(SubsystemHandle) -> Fut + Send,
    Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
{
    let mut redirected_subsystem_handle = subsystem_handle.delayed_clone();

    let join_handle = tokio::spawn({ async move { subsystem(subsystem_handle).await } });

    // Abort on drop
    guard.on_cancel({
        let abort_handle = join_handle.abort_handle();
        move || abort_handle.abort()
    });

    let result = match join_handle.await {
        Ok(Ok(())) => StopReason::Finish,
        Ok(Err(e)) => StopReason::Error(e),
        Err(e) => {
            tracing::error!("Subsystem <TODO: name> panicked: {}", e);
            StopReason::Panic
        }
    };

    guard.subsystem_ended(result);

    // Retrieve the handle that was passed into the subsystem.
    // Originally it was intended to pass the handle as reference, but due
    // to complications (https://stackoverflow.com/questions/77172947/async-lifetime-issues-of-pass-by-reference-parameters)
    // it was decided to pass ownership instead.
    //
    // It is still important that the handle does not leak out of the subsystem.
    let mut subsystem_handle = match redirected_subsystem_handle.try_recv() {
        Ok(s) => s,
        Err(_) => panic!("The SubsystemHandle object must not be leaked out of the subsystem!"),
    };

    // Wait for children to finish before we destroy the `SubsystemHandle` object.
    // Otherwise the children would be cancelled immediately.
    //
    // This is the main mechanism that forwards a cancellation to all the children.
    subsystem_handle.wait_for_children().await;
    drop(subsystem_handle);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::{
        sync::oneshot,
        time::{timeout, Duration},
    };

    use super::*;
    use crate::subsystem_handle::root_handle;

    fn create_result_and_guard() -> (oneshot::Receiver<StopReason>, AliveGuard) {
        let (sender, receiver) = oneshot::channel();

        let guard = AliveGuard::new();
        guard.on_finished({
            move |r| {
                sender.send(r).unwrap();
            }
        });

        (receiver, guard)
    }

    mod run_subsystem {

        use super::*;

        #[tokio::test]
        async fn finish() {
            let (mut result, guard) = create_result_and_guard();

            run_subsystem(
                |_| async { Result::<(), BoxedError>::Ok(()) },
                root_handle(),
                guard,
            )
            .await;

            assert!(matches!(result.try_recv(), Ok(StopReason::Finish)));
        }

        #[tokio::test]
        async fn panic() {
            let (mut result, guard) = create_result_and_guard();

            run_subsystem(
                |_| async {
                    panic!();
                },
                root_handle(),
                guard,
            )
            .await;

            assert!(matches!(result.try_recv(), Ok(StopReason::Panic)));
        }

        #[tokio::test]
        async fn error() {
            let (mut result, guard) = create_result_and_guard();

            run_subsystem(
                |_| async { Err(String::from("").into()) },
                root_handle(),
                guard,
            )
            .await;

            assert!(matches!(result.try_recv(), Ok(StopReason::Error(_))));
        }

        #[tokio::test]
        async fn cancelled_with_delay() {
            let (mut result, guard) = create_result_and_guard();

            let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

            let timeout_result = timeout(
                Duration::from_millis(100),
                run_subsystem(
                    |_| async move {
                        drop_sender.send(()).await.unwrap();
                        std::future::pending().await
                    },
                    root_handle(),
                    guard,
                ),
            )
            .await;

            assert!(timeout_result.is_err());
            drop(timeout_result);

            // Make sure we are executing the subsystem
            let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
                .await
                .unwrap();
            assert!(recv_result.is_some());

            // Make sure the subsystem got cancelled
            let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
                .await
                .unwrap();
            assert!(recv_result.is_none());

            assert!(matches!(result.try_recv(), Ok(StopReason::Cancelled)));
        }

        #[tokio::test]
        async fn cancelled_immediately() {
            let (mut result, guard) = create_result_and_guard();

            let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

            let _ = run_subsystem(
                |_| async move {
                    drop_sender.send(()).await.unwrap();
                    std::future::pending().await
                },
                root_handle(),
                guard,
            );

            // Make sure we are executing the subsystem
            let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
                .await
                .unwrap();
            assert!(recv_result.is_none());

            assert!(matches!(result.try_recv(), Ok(StopReason::Cancelled)));
        }
    }

    mod subsystem_runner {
        use crate::utils::JoinerToken;

        use super::*;

        #[tokio::test]
        async fn finish() {
            let (mut result, guard) = create_result_and_guard();

            let runner = SubsystemRunner::new(
                |_| async { Result::<(), BoxedError>::Ok(()) },
                root_handle(),
                guard,
            );

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Finish)));
        }

        #[tokio::test]
        async fn panic() {
            let (mut result, guard) = create_result_and_guard();

            let runner = SubsystemRunner::new(
                |_| async {
                    panic!();
                },
                root_handle(),
                guard,
            );

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Panic)));
        }

        #[tokio::test]
        async fn error() {
            let (mut result, guard) = create_result_and_guard();

            let runner = SubsystemRunner::new(
                |_| async { Err(String::from("").into()) },
                root_handle(),
                guard,
            );

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Error(_))));
        }

        #[tokio::test]
        async fn cancelled_with_delay() {
            let (mut result, guard) = create_result_and_guard();

            let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

            let runner = SubsystemRunner::new(
                |_| async move {
                    drop_sender.send(()).await.unwrap();
                    std::future::pending().await
                },
                root_handle(),
                guard,
            );

            // Make sure we are executing the subsystem
            let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
                .await
                .unwrap();
            assert!(recv_result.is_some());

            drop(runner);

            // Make sure the subsystem got cancelled
            let recv_result = timeout(Duration::from_millis(100), drop_receiver.recv())
                .await
                .unwrap();
            assert!(recv_result.is_none());

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Cancelled)));
        }

        #[tokio::test]
        async fn cancelled_immediately() {
            let (mut result, guard) = create_result_and_guard();

            let mut joiner_token = JoinerToken::new(|_| None);

            let _ = SubsystemRunner::new(
                {
                    let joiner_token = joiner_token.child_token(|_| None);
                    |_| async move {
                        let joiner_token = joiner_token;
                        std::future::pending().await
                    }
                },
                root_handle(),
                guard,
            );

            // Make sure the subsystem got cancelled
            timeout(Duration::from_millis(100), joiner_token.join_children())
                .await
                .unwrap();

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Cancelled)));
        }
    }
}
