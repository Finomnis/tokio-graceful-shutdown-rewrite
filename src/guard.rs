use crate::BoxedError;

#[derive(Debug)]
pub enum StopReason {
    Finish,
    Panic,
    Error(BoxedError),
    Cancelled,
}

pub struct AliveGuard {
    finished_callback: Option<Box<dyn FnOnce(StopReason)>>,
    cancelled_callback: Option<Box<dyn FnOnce()>>,
}

impl AliveGuard {
    pub fn new(finished_callback: impl FnOnce(StopReason) + 'static) -> Self {
        Self {
            finished_callback: Some(Box::new(finished_callback)),
            cancelled_callback: None,
        }
    }

    pub fn subsystem_ended(mut self, reason: StopReason) {
        let finished_callback = self.finished_callback.take().expect(
            "This should never happen. Please report this; it indicates a programming error.",
        );
        finished_callback(reason);
    }

    pub fn on_cancel(&mut self, cancelled_callback: impl FnOnce() + 'static) {
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
