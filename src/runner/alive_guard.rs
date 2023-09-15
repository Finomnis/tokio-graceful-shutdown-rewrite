use crate::StopReason;

pub(crate) struct AliveGuard {
    finished_callback: Option<Box<dyn FnOnce(StopReason) + Send>>,
    cancelled_callback: Option<Box<dyn FnOnce() + Send>>,
}

impl AliveGuard {
    pub(crate) fn new(finished_callback: impl FnOnce(StopReason) + 'static + Send) -> Self {
        Self {
            finished_callback: Some(Box::new(finished_callback)),
            cancelled_callback: None,
        }
    }

    pub(crate) fn subsystem_ended(mut self, reason: StopReason) {
        let finished_callback = self.finished_callback.take().expect(
            "This should never happen. Please report this; it indicates a programming error.",
        );
        finished_callback(reason);
    }

    pub(crate) fn on_cancel(&mut self, cancelled_callback: impl FnOnce() + 'static + Send) {
        assert!(self.cancelled_callback.is_none());
        self.cancelled_callback = Some(Box::new(cancelled_callback));
    }
}

impl Drop for AliveGuard {
    fn drop(&mut self) {
        if let Some(finished_callback) = self.finished_callback.take() {
            finished_callback(StopReason::Cancelled);
        }
        if let Some(cancelled_callback) = self.cancelled_callback.take() {
            cancelled_callback()
        }
    }
}
