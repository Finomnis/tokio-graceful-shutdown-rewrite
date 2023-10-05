//! This example demonstrates the basic usage pattern of this crate.
//!
//! It shows that subsystems get started, and when the program
//! gets shut down (by pressing Ctrl-C), the subsystems get shut down
//! gracefully.
//!
//! If custom arguments for the subsystem coroutines are required,
//! a struct has to be used instead, as seen in other examples.

use miette::Result;
use tokio::time::{sleep, Duration};
use tokio_graceful_shutdown_rewrite::{BoxedError, SubsystemHandle, Toplevel};

async fn subsys1(subsys: SubsystemHandle) -> Result<(), BoxedError> {
    tracing::info!("Subsystem1 started.");
    subsys.on_shutdown_requested().await;
    tracing::info!("Shutting down Subsystem1 ...");
    sleep(Duration::from_millis(400)).await;
    tracing::info!("Subsystem1 stopped.");
    Ok(())
}

async fn subsys2(subsys: SubsystemHandle) -> Result<(), BoxedError> {
    tracing::info!("Subsystem2 started.");
    subsys.on_shutdown_requested().await;
    tracing::info!("Shutting down Subsystem2 ...");
    sleep(Duration::from_millis(500)).await;
    tracing::info!("Subsystem2 stopped.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), BoxedError> {
    // Init logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    // Create toplevel
    Toplevel::new(|s| async move {
        s.start("Subsys1", subsys1);
        s.start("Subsys2", subsys2);
        Ok(())
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_millis(1000))
    .await
    .map_err(Into::into)
}
