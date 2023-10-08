mod nested_subsystem;
mod subsystem_builder;
mod subsystem_handle;

pub use subsystem_builder::SubsystemBuilder;
pub use subsystem_handle::SubsystemHandle;

pub(crate) use subsystem_handle::root_handle;

use crate::{utils::JoinerTokenRef, ErrTypeTraits};

use tokio_util::sync::CancellationToken;

pub struct NestedSubsystem {
    joiner: JoinerTokenRef,
    cancellation_token: CancellationToken,
}
