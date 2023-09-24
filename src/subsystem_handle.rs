use tokio_util::sync::CancellationToken;

use crate::utils::{remote_drop_collection::RemotelyDroppableItems, JoinerToken};

// All the things needed to manage nested subsystems and wait for cancellation
pub struct SubsystemHandle {
    cancellation_token: CancellationToken,
    joiner_token: JoinerToken,
}
