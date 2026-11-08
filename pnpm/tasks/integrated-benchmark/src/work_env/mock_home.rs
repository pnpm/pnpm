//! The homes of the revision mocks: the hosted store and metadata database
//! each one serves from.

use std::path::{Path, PathBuf};

/// The home of a revision's mock, in its bench dir. The fixture build's
/// hosted store and database carry this checkout's layout and metadata
/// schema, which the `pnpr` of another revision may refuse, so every
/// revision's mock gets a home of its own and serves what it proxies from
/// the shared cache. A scenario with packages of its own publishes them to
/// the mock once it is up.
pub(super) const REVISION_MOCK: &str = "revision-mock-home";

/// The home of a cold mock's warm origin, apart from the warm mock's so the
/// two processes never share a store or an `SQLite` file.
pub(super) const COLD_ORIGIN: &str = "cold-origin-home";

/// The home of a revision's mock: `name` in its bench dir, emptied, so its
/// `pnpr` creates the store and database afresh and never reopens what
/// another commit's `pnpr` created there earlier.
pub(super) fn revision_mock_home(bench_dir: &Path, name: &str) -> PathBuf {
    let home = bench_dir.join(name);
    match std::fs::remove_dir_all(&home) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => panic!("remove the stale mock home {home:?}: {err}"),
    }
    std::fs::create_dir_all(&home).expect("create the mock home");
    home
}
