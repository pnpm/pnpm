//! Unit tests for [`crate::installability::compute_skipped_snapshots`].

use crate::installability::InstallabilityHost;
use pnpm_lockfile::{
    ImporterDepVersion, LockfileResolution, PackageKey, PackageMetadata, PkgName, PkgNameVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef,
    TarballResolution,
};
use std::collections::HashMap;

// Per-test recording reporter. Its `Mutex<Vec<LogEvent>>` buffer is fn-local,
// so each `#[test]` captures into its own and concurrent tests never share or
// race on it. Each test names the helpers it drives, so every emitted helper is
// used and none needs a `dead_code` allow.
macro_rules! recording_reporter {
    ($($helper:ident),* $(,)?) => {
        static RECORDED_EVENTS: std::sync::Mutex<Vec<pnpm_reporter::LogEvent>> = std::sync::Mutex::new(Vec::new());

        struct RecordingReporter;
        impl pnpm_reporter::Reporter for RecordingReporter {
            fn emit(event: &pnpm_reporter::LogEvent) {
                RECORDED_EVENTS.lock().expect("RECORDED_EVENTS not poisoned").push(event.clone());
            }
        }

        $( recording_reporter!(@helper $helper); )*
    };

    (@helper take_events) => {
        fn take_events() -> Vec<pnpm_reporter::LogEvent> {
            std::mem::take(&mut *RECORDED_EVENTS.lock().expect("RECORDED_EVENTS not poisoned"))
        }
    };
    (@helper reset_events) => {
        fn reset_events() {
            RECORDED_EVENTS.lock().expect("RECORDED_EVENTS not poisoned").clear();
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `recording_reporter!` helper `",
            stringify!($unknown),
            "`; expected one of: take_events, reset_events",
        ));
    };
}

fn snapshot_key(name_at_version: &str) -> PackageKey {
    name_at_version.parse::<PkgNameVerPeer>().expect("valid package key")
}

fn synthetic_metadata(
    engines: Option<&[(&str, &str)]>,
    cpu: Option<&[&str]>,
    os: Option<&[&str]>,
    libc: Option<&[&str]>,
) -> PackageMetadata {
    // Tarball resolution — the installability check ignores the
    // resolution shape entirely, but every `PackageMetadata` must
    // carry one.
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: "https://example.test/pkg.tgz".to_string(),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        version: None,
        engines: engines.map(|entries| {
            entries.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
        }),
        cpu: cpu.map(|values| values.iter().map(|s| (*s).to_string()).collect()),
        os: os.map(|values| values.iter().map(|s| (*s).to_string()).collect()),
        libc: libc.map(|values| values.iter().map(|s| (*s).to_string()).collect()),
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn no_importers() -> HashMap<String, ProjectSnapshot> {
    HashMap::new()
}

/// A `.` root importer whose `dependencies` / `optionalDependencies`
/// entries resolve each `name@version` pair through the virtual store.
fn root_importer(
    dependencies: &[&str],
    optional_dependencies: &[&str],
) -> HashMap<String, ProjectSnapshot> {
    let importer = ProjectSnapshot {
        dependencies: importer_dep_map(dependencies),
        optional_dependencies: importer_dep_map(optional_dependencies),
        ..Default::default()
    };
    std::iter::once((".".to_string(), importer)).collect()
}

fn importer_dep_map(entries: &[&str]) -> Option<ResolvedDependencyMap> {
    if entries.is_empty() {
        return None;
    }
    let map = entries
        .iter()
        .map(|name_at_version| {
            let key = snapshot_key(name_at_version);
            let spec = ResolvedDependencySpec {
                specifier: "*".to_string(),
                version: ImporterDepVersion::Regular(key.suffix.clone()),
            };
            (key.name, spec)
        })
        .collect();
    Some(map)
}

fn snapshot_dep_map(entries: &[&str]) -> Option<HashMap<PkgName, SnapshotDepRef>> {
    if entries.is_empty() {
        return None;
    }
    let map = entries
        .iter()
        .map(|name_at_version| {
            let key = snapshot_key(name_at_version);
            (key.name, SnapshotDepRef::Plain(key.suffix))
        })
        .collect();
    Some(map)
}

fn host(node_version: &str, os: &'static str, cpu: &'static str) -> InstallabilityHost {
    InstallabilityHost {
        node_version: node_version.to_string(),
        node_detected: true,
        os,
        cpu,
        libc: "unknown",
        supported_architectures: None,
        engine_strict: false,
    }
}

mod optional_reachability;
mod platform_checks;
mod reporting;
mod runtimes;
