use super::is_threadsafe;

/// Pins that the `SQLite` the workspace links against is built
/// thread-safe, which every caller that opens a shared connection relies
/// on.
#[test]
fn the_bundled_build_is_threadsafe() {
    assert!(is_threadsafe());
}
