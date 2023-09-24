use std::future::Future;

use tokio_util::sync::CancellationToken;

use crate::{
    runner::{AliveGuard, SubsystemRunner},
    utils::{remote_drop_collection::RemotelyDroppableItems, JoinerToken},
    BoxedError,
};

// All the things needed to manage nested subsystems and wait for cancellation
pub struct SubsystemHandle {
    cancellation_token: CancellationToken,
    joiner_token: JoinerToken,
    children: RemotelyDroppableItems<SubsystemRunner>,
}

impl SubsystemHandle {
    pub fn start<Fut, Subsys>(&self, name: &str, subsystem: Subsys)
    where
        Subsys: 'static + FnOnce(&mut SubsystemHandle) -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
    {
        let alive_guard = AliveGuard::new();

        let child_handle = SubsystemHandle {
            cancellation_token: self.cancellation_token.child_token(),
            joiner_token: self.joiner_token.child_token(
                |e| Some(e), /* Forward error upwards. TODO: implement handling */
            ),
            children: RemotelyDroppableItems::new(),
        };

        let runner = SubsystemRunner::new(
            || async move {
                let mut subsystem_handle = child_handle;
                subsystem(&mut subsystem_handle).await
            },
            alive_guard.clone(),
        );

        // Shenanigans to juggle child ownership
        //
        // RACE CONDITION SAFETY:
        // If the subsystem ends before `on_finished` was able to be called, nothing bad happens.
        // alive_guard will keep the guard alive and the callback will only be called inside of its drop()
        // implementation.
        let child_dropper = self.children.insert(runner);
        alive_guard.on_finished(|d| drop(child_dropper));
    }
}
#[cfg(test)]
mod tests {

    use super::*;
    use crate::utils::JoinerToken;

    // TODO: Tests
    // #[test]
    // fn insert_and_drop() {}
}
