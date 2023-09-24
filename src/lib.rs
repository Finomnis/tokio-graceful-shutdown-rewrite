#![deny(unreachable_pub)]
#![deny(unsafe_code)]
#![allow(unused)] // TODO: remove this.

mod runner;
mod subsystem;
mod subsystem_handle;
mod utils;

pub use subsystem_handle::SubsystemHandle;

pub type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub enum StopReason {
    Finish,
    Panic,
    Error(BoxedError),
    Cancelled,
}
