mod nested_subsystem;
mod subsystem_handle;

pub(crate) use subsystem_handle::root_handle;
pub use subsystem_handle::SubsystemHandle;

use crate::utils::JoinerTokenRef;

use tokio_util::sync::CancellationToken;

pub struct NestedSubsystem {
    joiner: JoinerTokenRef,
    cancellation_token: CancellationToken,
}
