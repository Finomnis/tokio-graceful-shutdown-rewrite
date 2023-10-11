use crate::{errors::SubsystemJoinError, ErrTypeTraits};

use super::NestedSubsystem;

impl<ErrType: ErrTypeTraits> NestedSubsystem<ErrType> {
    pub async fn join(&self) -> Result<(), SubsystemJoinError<ErrType>> {
        self.joiner.join().await;

        let errors = self.errors.lock().unwrap().finish();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SubsystemJoinError::SubsystemsFailed(errors))
        }
    }

    pub fn initiate_shutdown(&self) {
        self.cancellation_token.cancel()
    }
}
