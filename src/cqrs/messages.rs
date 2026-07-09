/// Command message handled by [`crate::CommandBus`].
pub trait Command: Send + 'static {
    type Output: Send + 'static;
}

/// Query message handled by [`crate::QueryBus`].
pub trait Query: Send + 'static {
    type Output: Send + 'static;
}

/// Event message published through [`crate::EventBus`].
pub trait CqrsEvent: Clone + Send + Sync + 'static {}

impl<T> CqrsEvent for T where T: Clone + Send + Sync + 'static {}
