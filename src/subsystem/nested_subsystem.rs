use crate::{errors::SubsystemJoinError, ErrTypeTraits};

use super::NestedSubsystem;

impl<ErrType: ErrTypeTraits> NestedSubsystem<ErrType> {
    pub async fn join(&self) -> Result<(), SubsystemJoinError<ErrType>> {
        // TODO: implement error handling
        // Do it like in the Toplevel, where we have an mpsc that collects the errors
        self.joiner.join().await
    }

    pub fn initiate_shutdown(&self) {
        self.cancellation_token.cancel()
    }
}
