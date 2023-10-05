use std::{future::Future, sync::Arc, time::Duration};

use tokio::time::error::Elapsed;
use tokio_util::sync::CancellationToken;

use crate::{
    runner::{AliveGuard, SubsystemRunner},
    signal_handling::wait_for_signal,
    subsystem, BoxedError, ErrTypeTraits, NestedSubsystem, SubsystemHandle,
};

#[must_use = "This toplevel must be consumed by calling `handle_shutdown_requests` on it."]
pub struct Toplevel<ErrType: ErrTypeTraits = BoxedError> {
    root_handle: SubsystemHandle<ErrType>,
    toplevel_subsys: NestedSubsystem,
}

impl<ErrType: ErrTypeTraits> Toplevel<ErrType> {
    /// Creates a new Toplevel object.
    ///
    /// The Toplevel object is the base for everything else in this crate.
    #[allow(clippy::new_without_default)]
    pub fn new<Fut, Subsys>(subsystem: Subsys) -> Self
    where
        Subsys: 'static + FnOnce(SubsystemHandle<ErrType>) -> Fut + Send,
        Fut: 'static + Future<Output = ()> + Send,
    {
        let root_handle = subsystem::root_handle();

        let toplevel_subsys = root_handle.start_with_abs_name(Arc::from(""), move |s| async move {
            subsystem(s).await;
            Result::<(), ErrType>::Ok(())
        });

        Self {
            root_handle,
            toplevel_subsys,
        }
    }

    pub async fn handle_shutdown_requests(
        mut self,
        shutdown_timeout: Duration,
    ) -> Result<(), BoxedError> {
        tokio::select!(
            _ = self.toplevel_subsys.join() => {
                self.root_handle.initiate_shutdown();
                // TODO: error handling
                return Ok(());
            },
            _ = self.root_handle.on_shutdown_requested() => {
            }
        );

        match tokio::time::timeout(shutdown_timeout, self.toplevel_subsys.join()).await {
            Ok(()) => {
                tracing::info!("Shutdown finished.");
            }
            Err(_) => {
                tracing::error!("Shutdown timed out!");
            }
        }

        // TODO: error handling
        Ok(())
    }

    pub fn catch_signals(self) -> Self {
        let shutdown_token = self.root_handle.get_cancellation_token().clone();

        tokio::spawn(async move {
            wait_for_signal().await;
            shutdown_token.cancel();
        });

        self
    }

    #[doc(hidden)]
    pub fn get_shutdown_token(&self) -> CancellationToken {
        self.root_handle.get_cancellation_token()
    }
}
