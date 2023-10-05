mod nested_subsystem;
mod subsystem_handle;

use crate::utils::JoinerTokenRef;

pub(crate) use subsystem_handle::root_handle;
pub use subsystem_handle::SubsystemHandle;

pub struct NestedSubsystem {
    joiner: JoinerTokenRef,
}
