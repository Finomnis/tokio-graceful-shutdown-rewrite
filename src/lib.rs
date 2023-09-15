#![deny(unreachable_pub)]
#![deny(unsafe_code)]
#![allow(unused)] // TODO: remove this.

mod runner;
mod subsystem;
mod utils;

pub type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
enum StopReason {
    Finish,
    Panic,
    Error(BoxedError),
    Cancelled,
}
