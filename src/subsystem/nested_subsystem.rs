use super::NestedSubsystem;

impl NestedSubsystem {
    pub async fn join(&self) {
        self.joiner.join().await
    }

    pub fn initiate_shutdown(&self) {
        self.cancellation_token.cancel()
    }
}
