#![deny(unreachable_pub)]
#![allow(unused)] // TODO: remove this.

mod runner;
mod signal_handling;
mod subsystem;
mod toplevel;
mod utils;

pub use subsystem::NestedSubsystem;
pub use subsystem::SubsystemHandle;
pub use toplevel::Toplevel;

pub type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub enum StopReason {
    Finish,
    Panic,
    Error(BoxedError),
    Cancelled,
}
