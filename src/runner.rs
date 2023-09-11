use std::future::Future;

use crate::{BoxedError, StopReason};

mod alive_guard;
use self::alive_guard::AliveGuard;

pub(crate) async fn run_subsystem<Fut, Subsys>(subsystem: Subsys, mut guard: AliveGuard)
where
    Subsys: 'static + FnOnce() -> Fut + Send,
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

    use tokio::time::Duration;

    use super::*;

    fn create_result_and_guard() -> (Arc<Mutex<Option<StopReason>>>, AliveGuard) {
        let result = Arc::new(Mutex::new(None));

        let guard = AliveGuard::new({
            let result = Arc::clone(&result);
            move |r| {
                let mut result = result.lock().unwrap();
                *result = Some(r);
            }
        });

        (result, guard)
    }

    #[tokio::test]
    async fn finish() {
        let (result, guard) = create_result_and_guard();

        run_subsystem(|| async { Result::<(), BoxedError>::Ok(()) }, guard).await;

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Finish)
        ));
    }

    #[tokio::test]
    async fn panic() {
        let (result, guard) = create_result_and_guard();

        run_subsystem(
            || async {
                panic!();
            },
            guard,
        )
        .await;

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Panic)
        ));
    }

    #[tokio::test]
    async fn error() {
        let (result, guard) = create_result_and_guard();

        run_subsystem(|| async { Err(String::from("").into()) }, guard).await;

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Error(_))
        ));
    }

    #[tokio::test]
    async fn cancelled_with_delay() {
        let (result, guard) = create_result_and_guard();

        let (drop_sender, mut drop_receiver) = tokio::sync::mpsc::channel::<()>(1);

        let timeout_result = tokio::time::timeout(
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
        let recv_result = tokio::time::timeout(Duration::from_millis(100), drop_receiver.recv())
            .await
            .unwrap();
        assert!(recv_result.is_some());

        // Make sure the subsystem got cancelled
        let recv_result = tokio::time::timeout(Duration::from_millis(100), drop_receiver.recv())
            .await
            .unwrap();
        assert!(recv_result.is_none());

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Cancelled)
        ));
    }

    #[tokio::test]
    async fn cancelled_immediately() {
        let (result, guard) = create_result_and_guard();

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
        let recv_result = tokio::time::timeout(Duration::from_millis(100), drop_receiver.recv())
            .await
            .unwrap();
        assert!(recv_result.is_none());

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Cancelled)
        ));
    }
}
