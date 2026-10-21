/// Whether a file lock excludes every other handle, or only the handles
/// that want to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Exclusive,
    Shared,
}
