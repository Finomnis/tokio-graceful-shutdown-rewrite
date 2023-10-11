use std::{
    future::Future,
    sync::{mpsc, Arc},
    time::Duration,
};

use atomic::Atomic;
use tokio_util::sync::CancellationToken;

use crate::{
    errors::{GracefulShutdownError, SubsystemError},
    signal_handling::wait_for_signal,
    subsystem::{self, ErrorActions},
    BoxedError, ErrTypeTraits, ErrorAction, NestedSubsystem, SubsystemHandle,
};

#[must_use = "This toplevel must be consumed by calling `handle_shutdown_requests` on it."]
pub struct Toplevel<ErrType: ErrTypeTraits = BoxedError> {
    root_handle: SubsystemHandle<ErrType>,
    toplevel_subsys: NestedSubsystem<ErrType>,
    errors: mpsc::Receiver<SubsystemError<ErrType>>,
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
        let (error_sender, errors) = mpsc::channel();

        let root_handle = subsystem::root_handle(move |e| {
            match &e {
                SubsystemError::Panicked(name) => {
                    tracing::error!("Uncaught panic from subsytem '{name}'.")
                }
                SubsystemError::Failed(name, e) => {
                    tracing::error!("Uncaught error from subsystem '{name}': {e}",)
                }
            };

            if let Err(mpsc::SendError(e)) = error_sender.send(e) {
                tracing::warn!("An error got dropped: {e:?}");
            };
        });

        let toplevel_subsys = root_handle.start_with_abs_name(
            Arc::from(""),
            move |s| async move {
                subsystem(s).await;
                Result::<(), ErrType>::Ok(())
            },
            ErrorActions {
                on_failure: Atomic::new(ErrorAction::Forward),
                on_panic: Atomic::new(ErrorAction::Forward),
            },
        );

        Self {
            root_handle,
            toplevel_subsys,
            errors,
        }
    }

    pub async fn handle_shutdown_requests(
        self,
        shutdown_timeout: Duration,
    ) -> Result<(), GracefulShutdownError<ErrType>> {
        let collect_errors = move || {
            let mut errors = vec![];
            while let Ok(e) = self.errors.try_recv() {
                errors.push(e);
            }
            drop(self.errors);
            errors.into_boxed_slice()
        };

        tokio::select!(
            _ = self.toplevel_subsys.join() => {
                tracing::info!("All subsystems finished.");

                // Not really necessary, but for good measure.
                self.root_handle.initiate_shutdown();

                let errors = collect_errors();
                let result = if errors.is_empty() {
                    Ok(())
                } else {
                    Err(GracefulShutdownError::SubsystemsFailed(errors))
                };
                return result;
            },
            _ = self.root_handle.on_shutdown_requested() => {
                tracing::info!("Shutting down ...");
            }
        );

        match tokio::time::timeout(shutdown_timeout, self.toplevel_subsys.join()).await {
            Ok(Ok(())) => {
                let errors = collect_errors();
                if errors.is_empty() {
                    tracing::info!("Shutdown finished.");
                    Ok(())
                } else {
                    tracing::warn!("Shutdown finished with errors.");
                    Err(GracefulShutdownError::SubsystemsFailed(errors))
                }
            }
            Ok(Err(_)) => {
                // This can't happen because the toplevel subsys doesn't catch any errors; it only forwards them.
                unreachable!();
            }
            Err(_) => {
                tracing::error!("Shutdown timed out!");
                Err(GracefulShutdownError::ShutdownTimeout(collect_errors()))
            }
        }
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
    pub fn get_shutdown_token(&self) -> &CancellationToken {
        self.root_handle.get_cancellation_token()
    }
}
