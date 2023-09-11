use std::future::Future;

use crate::BoxedError;

#[derive(Debug)]
pub enum StopReason {
    Finish,
    Panic,
    Error(BoxedError),
    Cancelled,
}

pub struct AliveGuard {
    finished_callback: Option<Box<dyn FnOnce(StopReason)>>,
    cancelled_callback: Option<Box<dyn FnOnce()>>,
}

impl AliveGuard {
    pub fn new(finished_callback: impl FnOnce(StopReason) + 'static) -> Self {
        Self {
            finished_callback: Some(Box::new(finished_callback)),
            cancelled_callback: None,
        }
    }

    pub fn subsystem_ended(mut self, reason: StopReason) {
        let finished_callback = self.finished_callback.take().expect(
            "This should never happen. Please report this; it indicates a programming error.",
        );
        finished_callback(reason);
    }

    pub fn on_cancel(&mut self, cancelled_callback: impl FnOnce() + 'static) {
        assert!(self.cancelled_callback.is_none());
        self.cancelled_callback = Some(Box::new(cancelled_callback));
    }
}

impl Drop for AliveGuard {
    fn drop(&mut self) {
        if let Some(finished_callback) = self.finished_callback.take() {
            finished_callback(StopReason::Cancelled);
        }
        if let Some(cancelled_callback) = self.cancelled_callback.take() {
            cancelled_callback()
        }
    }
}

pub async fn run_subsystem<Fut, Subsys>(subsystem: Subsys, mut guard: AliveGuard)
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

/* need:
- function that runs on all exit paths of the subsystem
    - that also has access to the locked parent, to remove itself from the list of children
- is list of children really important?
    - atomic counter could work
    - but what about error propagation?

- error propagation maybe not necessary.
    - register closure that will be executed on error/shutdown of the child
    - every subsystem can have 'shutdown triggers' attached to it

fixed facts:
- subsystems will never change their parent

*/

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
    async fn run_subsystem_finish() {
        let (result, guard) = create_result_and_guard();

        run_subsystem(|| async { Result::<(), BoxedError>::Ok(()) }, guard).await;

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Finish)
        ));
    }

    #[tokio::test]
    async fn run_subsystem_panic() {
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
    async fn run_subsystem_error() {
        let (result, guard) = create_result_and_guard();

        run_subsystem(|| async { Err(String::from("").into()) }, guard).await;

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Error(_))
        ));
    }

    #[tokio::test]
    async fn run_subsystem_cancelled_with_delay() {
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
    async fn run_subsystem_cancelled_immediately() {
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
