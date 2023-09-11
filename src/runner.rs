use std::future::Future;

use crate::{
    guard::{AliveGuard, StopReason},
    BoxedError,
};

async fn run_subsystem<Fut, Subsys>(subsystem: Subsys, mut guard: AliveGuard)
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

    use tokio::time::{sleep, Duration};

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
    async fn run_subsystem_cancelled_immediately() {
        let (result, guard) = create_result_and_guard();

        let _ = run_subsystem(|| async { std::future::pending().await }, guard);

        assert!(matches!(
            result.lock().unwrap().take(),
            Some(StopReason::Error(_))
        ));
    }
}
