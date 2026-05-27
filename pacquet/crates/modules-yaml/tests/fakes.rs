//! Pacquet-side tests that use dependency injection to drive I/O outcomes
//! and time-dependent branches that are awkward or impossible to provoke
//! with the real filesystem. Each fake implements only the capability
//! trait the function under test consumes, so a read fake never has to
//! declare `write`. This is the interface-segregation refinement of the
//! lumped `FsApi` pattern at
//! <https://github.com/KSXGitHub/parallel-disk-usage/blob/2aa39917f9/src/app/hdd.rs#L25-L35>.

use chrono::{TimeZone, Utc};
use pacquet_modules_yaml::{
    Clock, DepPath, FsCreateDirAll, FsReadToString, FsWrite, Modules, read_modules_manifest,
    write_modules_manifest,
};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::{path::Path, time::SystemTime};
use text_block_macros::text_block;

/// `read_modules_manifest` should map a non-`NotFound` I/O error from
/// `read_to_string` to `ReadModulesError::ReadFile`.
#[test]
fn read_propagates_non_not_found_io_error() {
    use std::io;

    struct FailingRead;
    impl FsReadToString for FailingRead {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "mocked"))
        }
    }
    impl Clock for FailingRead {
        fn now() -> SystemTime {
            unreachable!("clock must not be called when read_to_string fails");
        }
    }

    let err = "/dev/null/unused"
        .pipe(Path::new)
        .pipe(read_modules_manifest::<FailingRead>)
        .expect_err("expected error");
    eprintln!("error: {err}");
    assert!(matches!(err, pacquet_modules_yaml::ReadModulesError::ReadFile { .. }));
}

/// `read_modules_manifest` should surface a YAML parse failure as
/// `ReadModulesError::ParseYaml`.
#[test]
fn read_propagates_parse_error() {
    use std::io;

    struct BadYamlContent;
    impl FsReadToString for BadYamlContent {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Ok("{ this is not valid yaml or json".to_string())
        }
    }
    impl Clock for BadYamlContent {
        fn now() -> SystemTime {
            unreachable!("clock must not be called when YAML parsing fails");
        }
    }

    let err = "/dev/null/unused"
        .pipe(Path::new)
        .pipe(read_modules_manifest::<BadYamlContent>)
        .expect_err("expected error");
    eprintln!("error: {err}");
    assert!(matches!(err, pacquet_modules_yaml::ReadModulesError::ParseYaml { .. }));
}

/// A YAML document that parses to `null` should yield `Ok(None)`, matching
/// upstream's `if (!modulesRaw) return modulesRaw;` at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L55>.
#[test]
fn read_returns_none_for_null_document() {
    use std::io;

    struct NullDocContent;
    impl FsReadToString for NullDocContent {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Ok("null\n".to_string())
        }
    }
    impl Clock for NullDocContent {
        fn now() -> SystemTime {
            unreachable!("clock must not be called when document is null");
        }
    }

    let result = "/dev/null/unused"
        .pipe(Path::new)
        .pipe(read_modules_manifest::<NullDocContent>)
        .expect("read manifest");
    assert_eq!(result, None);
}

/// `write_modules_manifest` should map a `create_dir_all` failure to
/// `WriteModulesError::CreateDir`. The fake still has to implement
/// `FsWrite` because the function bound includes it, but the body asserts
/// that `write` is never reached on this code path.
#[test]
fn write_propagates_create_dir_error() {
    use std::io;

    struct FailingMkdir;
    impl FsCreateDirAll for FailingMkdir {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "mocked"))
        }
    }
    impl FsWrite for FailingMkdir {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            unreachable!("write must not be called when create_dir_all fails");
        }
    }

    let modules_dir = Path::new("/dev/null/unused");
    let err = write_modules_manifest::<FailingMkdir>(modules_dir, Modules::default())
        .expect_err("expected error");
    eprintln!("error: {err}");
    assert!(matches!(err, pacquet_modules_yaml::WriteModulesError::CreateDir { .. }));
}

/// `write_modules_manifest` should map a `write` failure to
/// `WriteModulesError::WriteFile` after `create_dir_all` succeeds.
#[test]
fn write_propagates_write_error() {
    use std::io;

    struct FailingWrite;
    impl FsCreateDirAll for FailingWrite {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for FailingWrite {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            Err(io::Error::other("mocked write failure"))
        }
    }

    let modules_dir = Path::new("/dev/null/unused");
    let err = write_modules_manifest::<FailingWrite>(modules_dir, Modules::default())
        .expect_err("expected error");
    eprintln!("error: {err}");
    assert!(matches!(err, pacquet_modules_yaml::WriteModulesError::WriteFile { .. }));
}

/// `LayoutVersion` is a unit type pinned to `5`. A manifest whose
/// `layoutVersion` is any other number must fail at parse time. This is
/// stricter than upstream's `readModules`, which accepts any number and
/// defers the decision to `checkCompatibility` at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/deps-installer/src/install/checkCompatibility/index.ts#L18-L22>;
/// the end-to-end behavior matches because both code paths reject
/// incompatible manifests.
#[test]
fn read_rejects_incompatible_layout_version() {
    use std::io;

    struct LegacyVersion;
    impl FsReadToString for LegacyVersion {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Ok("layoutVersion: 4\n".to_string())
        }
    }
    impl Clock for LegacyVersion {
        fn now() -> SystemTime {
            unreachable!("clock must not be called when layout version is rejected");
        }
    }

    let err = "/dev/null/unused"
        .pipe(Path::new)
        .pipe(read_modules_manifest::<LegacyVersion>)
        .expect_err("expected error");
    eprintln!("error: {err}");
    assert!(matches!(err, pacquet_modules_yaml::ReadModulesError::ParseYaml { .. }));
}

/// `ignoredBuilds` deserializes into an [`IndexSet`], mirroring upstream's
/// `new Set<DepPath>(modulesRaw.ignoredBuilds)` normalization at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L64>.
/// Duplicates are dropped, and insertion order is preserved so a
/// write-after-read round-trip leaves the on-disk array byte-stable
/// against an upstream-written manifest.
///
/// [`IndexSet`]: indexmap::IndexSet
#[test]
fn ignored_builds_dedups_and_preserves_insertion_order() {
    use std::io;

    struct DupIgnored;
    impl FsReadToString for DupIgnored {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Ok(text_block! {
                "layoutVersion: 5"
                "ignoredBuilds:"
                "  - /b@1"
                "  - /a@1"
                "  - /b@1"
                "  - /c@1"
                "  - /a@1"
            }
            .to_string())
        }
    }
    impl Clock for DupIgnored {
        fn now() -> SystemTime {
            SystemTime::UNIX_EPOCH
        }
    }

    let manifest = "/dev/null/unused"
        .pipe(Path::new)
        .pipe(read_modules_manifest::<DupIgnored>)
        .expect("read manifest")
        .expect("manifest exists");
    let ignored: Vec<&str> = manifest
        .ignored_builds
        .as_ref()
        .expect("ignored_builds present")
        .iter()
        .map(DepPath::as_str)
        .collect();
    assert_eq!(ignored, ["/b@1", "/a@1", "/c@1"]);
}

/// `read_modules_manifest` fills in a missing `prunedAt` from the
/// injected [`Clock`] capability, formatting it as an HTTP date.
/// Mirrors upstream's `if (!modules.prunedAt) modules.prunedAt = new
/// Date().toUTCString()` at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L98-L99>.
/// This test pins the formatted output by faking the clock to a known
/// instant, since `SystemTime::now()` is otherwise non-deterministic.
#[test]
fn read_fills_pruned_at_from_clock_when_missing() {
    use std::io;

    struct FakeClock;
    impl FsReadToString for FakeClock {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Ok("layoutVersion: 5\n".to_string())
        }
    }
    impl Clock for FakeClock {
        fn now() -> SystemTime {
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap().into()
        }
    }

    let manifest = "/dev/null/unused"
        .pipe(Path::new)
        .pipe(read_modules_manifest::<FakeClock>)
        .expect("read manifest")
        .expect("manifest exists");
    assert_eq!(manifest.pruned_at, "Thu, 01 Jan 2026 00:00:00 GMT");
}
