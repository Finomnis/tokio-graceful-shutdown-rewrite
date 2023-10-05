use super::NestedSubsystem;

impl NestedSubsystem {
    pub async fn join(&self) {
        self.joiner.join().await
    }
}
