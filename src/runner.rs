//! The SubsystemRunner is a little tricky, so here some explanation.
//!
//! A two-layer `tokio::spawn` is required to make this work reliably; the inner `spawn` is the actual subsystem,
//! and the outer `spawn` carries out the duty of propagating the `StopReason`.
//!
//! Further, everything in here reacts properly to being dropped, including the `AliveGuard` (propagating `StopReason::Cancel` in that case)
//! and runner itself, who cancels the subsystem on drop.

use std::future::Future;

use crate::{BoxedError, StopReason};

mod alive_guard;
pub(crate) use self::alive_guard::AliveGuard;

pub(crate) struct SubsystemRunner {
    aborthandle: tokio::task::AbortHandle,
}

impl SubsystemRunner {
    pub(crate) fn new<Fut, Subsys>(subsystem: Subsys, mut guard: AliveGuard) -> Self
    where
        Subsys: 'static + FnOnce() -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
    {
        let aborthandle = tokio::spawn(run_subsystem(subsystem, guard)).abort_handle();
        SubsystemRunner { aborthandle }
    }
}

impl Drop for SubsystemRunner {
    fn drop(&mut self) {
        self.aborthandle.abort()
    }
}

async fn run_subsystem<Fut, Subsys>(subsystem: Subsys, mut guard: AliveGuard)
where
    Subsys: 'static + FnOnce() -> Fut,
    Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
{
    let join_handle = tokio::spawn(subsystem());

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
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::{
        sync::oneshot,
        time::{timeout, Duration},
    };

    use super::*;

    fn create_result_and_guard() -> (oneshot::Receiver<StopReason>, AliveGuard) {
        let (sender, receiver) = oneshot::channel();

        let guard = AliveGuard::new({
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

            run_subsystem(|| async { Result::<(), BoxedError>::Ok(()) }, guard).await;

            assert!(matches!(result.try_recv(), Ok(StopReason::Finish)));
        }

        #[tokio::test]
        async fn panic() {
            let (mut result, guard) = create_result_and_guard();

            run_subsystem(
                || async {
                    panic!();
                },
                guard,
            )
            .await;

            assert!(matches!(result.try_recv(), Ok(StopReason::Panic)));
        }

        #[tokio::test]
        async fn error() {
            let (mut result, guard) = create_result_and_guard();

            run_subsystem(|| async { Err(String::from("").into()) }, guard).await;

            assert!(matches!(result.try_recv(), Ok(StopReason::Error(_))));
        }

        #[tokio::test]
        async fn cancelled_with_delay() {
            let (mut result, guard) = create_result_and_guard();

            let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

            let timeout_result = timeout(
                Duration::from_millis(100),
                run_subsystem(
                    {
                        || async move {
                            drop_sender.send(()).await.unwrap();
                            std::future::pending().await
                        }
                    },
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
                {
                    || async move {
                        drop_sender.send(()).await.unwrap();
                        std::future::pending().await
                    }
                },
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

            let runner = SubsystemRunner::new(|| async { Result::<(), BoxedError>::Ok(()) }, guard);

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Finish)));
        }

        #[tokio::test]
        async fn panic() {
            let (mut result, guard) = create_result_and_guard();

            let runner = SubsystemRunner::new(
                || async {
                    panic!();
                },
                guard,
            );

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Panic)));
        }

        #[tokio::test]
        async fn error() {
            let (mut result, guard) = create_result_and_guard();

            let runner = SubsystemRunner::new(|| async { Err(String::from("").into()) }, guard);

            let result = timeout(Duration::from_millis(200), result).await.unwrap();
            assert!(matches!(result, Ok(StopReason::Error(_))));
        }

        #[tokio::test]
        async fn cancelled_with_delay() {
            let (mut result, guard) = create_result_and_guard();

            let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

            let runner = SubsystemRunner::new(
                {
                    || async move {
                        drop_sender.send(()).await.unwrap();
                        std::future::pending().await
                    }
                },
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

            let mut joiner_token = JoinerToken::new();

            let _ = SubsystemRunner::new(
                {
                    let joiner_token = joiner_token.create_child_token();
                    || async move {
                        let joiner_token = joiner_token;
                        std::future::pending().await
                    }
                },
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
