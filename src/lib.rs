#![deny(unreachable_pub)]
//#![deny(missing_docs)]
#![doc(
    issue_tracker_base_url = "https://github.com/Finomnis/tokio-graceful-shutdown/issues",
    test(no_crate_inject, attr(deny(warnings))),
    test(attr(allow(dead_code)))
)]
#![allow(unused)] // TODO: remove this.

pub type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// A collection of traits a custom error has to fulfill in order to be
/// usable as the `ErrType` of [Toplevel].
pub trait ErrTypeTraits:
    std::fmt::Debug + std::fmt::Display + 'static + Send + Sync + Sized
{
}
impl<T> ErrTypeTraits for T where
    T: std::fmt::Debug + std::fmt::Display + 'static + Send + Sync + Sized
{
}

pub mod errors;

mod error_action;
mod future_ext;
mod into_subsystem;
mod runner;
mod signal_handling;
mod subsystem;
mod toplevel;
mod utils;

use std::fmt::Display;

pub use error_action::ErrorAction;
pub use future_ext::FutureExt;
pub use into_subsystem::IntoSubsystem;
pub use subsystem::NestedSubsystem;
pub use subsystem::SubsystemBuilder;
pub use subsystem::SubsystemHandle;
pub use toplevel::Toplevel;
