/// Graph state snapshot contract.
///
/// Implementors must be cheaply cloneable so the executor can fan-out branches
/// and checkpoint after each node without moving ownership unexpectedly.
pub trait State: Clone + Send + Sync + 'static {}

impl<T> State for T where T: Clone + Send + Sync + 'static {}
