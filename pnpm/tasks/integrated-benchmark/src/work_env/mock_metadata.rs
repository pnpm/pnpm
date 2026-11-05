//! The metadata databases the revision mocks read.

use super::WorkEnv;
use crate::cli_args::BenchmarkScenario;
use std::path::{Path, PathBuf};

/// The metadata database a revision's mock reads, in its bench dir. The
/// fixture build's database carries this checkout's metadata schema, which
/// the `pnpr` of another revision may refuse, so every revision's mock gets a
/// database of its own and serves what it proxies from the shared cache. The
/// peer-heavy scenario's fixtures are seeded into the fixture database, so
/// its mocks read that one.
pub(super) const REVISION_MOCK: &str = "revision-mock-metadata.sqlite";

/// The metadata database of a cold mock's warm origin, apart from the warm
/// mock's so the two processes never share one `SQLite` file.
pub(super) const COLD_ORIGIN: &str = "cold-origin-metadata.sqlite";

impl WorkEnv {
    /// The metadata database a revision's mock reads: `file` in its bench
    /// dir, created afresh by that revision's `pnpr`, so a database another
    /// commit's `pnpr` created there earlier is never reopened. A scenario
    /// with its own fixture packages reads the database they were seeded
    /// into instead, `None`.
    pub(super) fn revision_mock_metadata(&self, bench_dir: &Path, file: &str) -> Option<PathBuf> {
        if self.options.selection.scenario.is_some_and(BenchmarkScenario::uses_peer_heavy_fixture) {
            return None;
        }
        let database = bench_dir.join(file);
        for suffix in ["", "-wal", "-shm"] {
            let mut path = database.clone().into_os_string();
            path.push(suffix);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => panic!("remove the stale mock database {path:?}: {err}"),
            }
        }
        Some(database)
    }
}
