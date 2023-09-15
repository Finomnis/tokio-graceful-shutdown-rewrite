use std::future::Future;

use crate::{
    runner::{AliveGuard, SubsystemRunner},
    BoxedError,
};

pub(crate) struct SubSystem {
    runner: SubsystemRunner,
}

impl SubSystem {
    async fn new<Fut, Subsys>(subsystem: Subsys) -> Self
    where
        Subsys: 'static + FnOnce() -> Fut + Send,
        Fut: 'static + Future<Output = Result<(), BoxedError>> + Send,
    {
        Self {
            runner: SubsystemRunner::new(subsystem, AliveGuard::new(|_| {})),
        }
    }
}

// #[cfg(test)]
// mod tests {

//     use super::*;
//     use crate::utils::JoinerToken;

//     #[tokio::test]
//     async fn cancelled_with_delay() {
//         Subsystem::new();
//         panic!();
//     }
// }
