use std::{future::Future, time::Duration};

use tokio::time::error::Elapsed;
use tokio_util::sync::CancellationToken;

use crate::{
    runner::{AliveGuard, SubsystemRunner},
    signal_handling::wait_for_signal,
    subsystem, BoxedError, NestedSubsystem, SubsystemHandle,
};

pub struct Toplevel {
    root_handle: SubsystemHandle,
    toplevel_subsys: NestedSubsystem,
}

impl Toplevel {
    pub fn new<Fut, Subsys>(subsystem: Subsys) -> Self
    where
        Subsys: 'static + FnOnce(SubsystemHandle) -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
    {
        let root_handle = subsystem::root_handle();

        let toplevel_subsys = root_handle.start("", subsystem);

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
                panic!("Shutdown timed out!")
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
