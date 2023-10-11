mod nested_subsystem;
mod subsystem_builder;
mod subsystem_handle;

use std::marker::PhantomData;

pub use subsystem_builder::SubsystemBuilder;
pub use subsystem_handle::SubsystemHandle;

pub(crate) use subsystem_handle::root_handle;

use crate::{utils::JoinerTokenRef, ErrTypeTraits};

use tokio_util::sync::CancellationToken;

pub struct NestedSubsystem<ErrType: ErrTypeTraits> {
    joiner: JoinerTokenRef,
    cancellation_token: CancellationToken,
    _phantom: PhantomData<fn() -> ErrType>,
}
