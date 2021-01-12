use thiserror::Error;

#[derive(Debug, Error)]
pub enum TryRecvError {
    /// The stream may produce an item at a later time
    #[error("TryRecvError::Pending")]
    Pending,
    /// The stream is closed, and will never produce an item
    #[error("TryRecvError::Rejected")]
    Rejected,
}
