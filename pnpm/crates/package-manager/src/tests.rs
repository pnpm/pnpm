use super::script_thread_count;
use pacquet_config::Config;

/// A [`Config`] pinned to the project-local virtual store.
///
/// The pin is what keeps a test's installs inside its own tempdir. A
/// [`Config`] built here has never been through
/// [`Config::current`](pacquet_config::Config::current), so assigning
/// `store_dir` does not re-derive `global_virtual_store_dir` with it:
/// that field still names the *machine's* store. On the shipped default
/// — the shared store — an install through such a config would leave
/// test packages in the developer's real store and share those slots
/// with every concurrently running test. Every test config in this
/// crate starts here; the ones that exercise the shared store turn it
/// back on and point `global_virtual_store_dir` inside their own
/// tempdir.
pub(crate) fn project_local_config() -> Config {
    Config { enable_global_virtual_store: false, ..Config::new() }
}

#[test]
fn script_threads_are_bounded_by_work_and_the_safety_cap() {
    assert_eq!(script_thread_count(0, 0), 1);
    assert_eq!(script_thread_count(8, 3), 3);
    assert_eq!(script_thread_count(u32::MAX, usize::MAX), 256);
}
