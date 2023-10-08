use std::{borrow::Cow, future::Future, marker::PhantomData, sync::Arc};

use crate::{ErrTypeTraits, NestedSubsystem, SubsystemHandle};

pub struct SubsystemBuilder<'a, ErrType, Err, Fut, Subsys>
where
    ErrType: ErrTypeTraits,
    Subsys: 'static + FnOnce(SubsystemHandle<ErrType>) -> Fut + Send,
    Fut: 'static + Future<Output = Result<(), Err>> + Send,
    Err: Into<ErrType>,
{
    pub(crate) name: Cow<'a, str>,
    pub(crate) subsystem: Subsys,
    _phantom: PhantomData<(fn() -> (Fut, ErrType, Err))>,
}

impl<'a, ErrType, Err, Fut, Subsys> SubsystemBuilder<'a, ErrType, Err, Fut, Subsys>
where
    ErrType: ErrTypeTraits,
    Subsys: 'static + FnOnce(SubsystemHandle<ErrType>) -> Fut + Send,
    Fut: 'static + Future<Output = Result<(), Err>> + Send,
    Err: Into<ErrType>,
{
    pub fn new(name: impl Into<Cow<'a, str>>, subsystem: Subsys) -> Self {
        Self {
            name: name.into(),
            subsystem,
            _phantom: Default::default(),
        }
    }
}
