use super::{
    Arc, EngineMode, InstallOptions, install_options_for, rebuild_options, run_install_inner,
};
use async_trait::async_trait;
use indexmap::IndexMap;
use pnpm_hooks::{
    HookContext, HookError, PnpmfileHooks, PreResolutionHookContext, PreResolutionHookLogger,
    ReadPackageResult,
};
use serde_json::Value;
use std::{path::Path, sync::Mutex};

const FOO: &str = "@pnpm.e2e/foo";
const BAR: &str = "@pnpm.e2e/bar";

/// Stands in for the JS adapters in `crate::hooks`: an identity
/// `readPackage` the engine cannot fingerprint, recording the name of
/// every manifest it receives.
#[derive(Default)]
struct RecordingReadPackageHook {
    names: Mutex<Vec<String>>,
}

#[async_trait]
impl PnpmfileHooks for RecordingReadPackageHook {
    async fn read_package(
        &self,
        pkg: Value,
        _: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        if let Some(name) = pkg.get("name").and_then(Value::as_str) {
            self.names
                .lock()
                .expect("names lock")
                .push(name.to_string());
        }
        Ok(Arc::new(pkg))
    }

    async fn after_all_resolved(&self, _: Value, _: HookContext) -> Result<Value, HookError> {
        Ok(Value::Null)
    }

    async fn pre_resolution(&self, _: PreResolutionHookContext, _: PreResolutionHookLogger) {}

    async fn filter_log(&self, _: Value, _: HookContext) -> bool {
        true
    }

    async fn has_read_package(&self) -> Result<bool, HookError> {
        Ok(true)
    }

    async fn untracked_read_package_hook(&self) -> Result<Option<bool>, HookError> {
        Ok(Some(true))
    }
}

fn hooked_install_options(
    temp_dir: &Path,
    dependencies: &[&str],
    checksum: Option<&str>,
) -> InstallOptions {
    let dependencies: serde_json::Map<String, Value> = dependencies
        .iter()
        .map(|name| (name.to_string(), Value::from("100.0.0")))
        .collect();
    let mut options = install_options_for(
        temp_dir,
        "project",
        serde_json::json!({ "dependencies": dependencies }),
    );
    options.read_package_hook_checksum = checksum.map(str::to_string);
    options
}

/// Runs one install through a fresh hook and returns the names of the
/// manifests the hook received.
fn install_with_hook(options: &InstallOptions) -> Vec<String> {
    let hook = Arc::new(RecordingReadPackageHook::default());
    run_install_inner(
        options,
        Some(Arc::clone(&hook) as Arc<dyn PnpmfileHooks>),
        EngineMode::Install(None),
    )
    .expect("install");
    let names = hook.names
        .lock()
        .expect("names lock")
        .clone();
    eprintln!("readPackage received: {names:?}");
    names
}

fn read_lockfile(options: &InstallOptions) -> String {
    let lockfile = std::fs::read_to_string(Path::new(&options.dir).join("pnpm-lock.yaml"))
        .expect("read lockfile");
    eprintln!("lockfile:\n{lockfile}");
    lockfile
}

#[test]
fn checksummed_read_package_hook_reuses_the_lockfile_until_the_checksum_changes() {
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let options = hooked_install_options(temp_dir.path(), &[FOO], Some("hooks-1"));
    assert!(
        install_with_hook(&options)
            .iter()
            .any(|name| name == FOO),
        "the first install resolves through the hook",
    );
    let lockfile = read_lockfile(&options);
    assert!(lockfile.contains("pnpmfileChecksum: hooks-1"));
    assert!(!lockfile.contains("untrackedPnpmfileReadPackageHook"));

    let options = hooked_install_options(temp_dir.path(), &[FOO, BAR], Some("hooks-1"));
    let names = install_with_hook(&options);
    assert!(names.iter().any(|name| name == BAR));
    assert!(!names.iter().any(|name| name == FOO), "the resolved dependency is reused");

    let options = hooked_install_options(temp_dir.path(), &[FOO, BAR], Some("hooks-2"));
    let names = install_with_hook(&options);
    assert!(names.iter().any(|name| name == FOO), "a changed checksum resolves again");
    assert!(read_lockfile(&options).contains("pnpmfileChecksum: hooks-2"));
}

#[test]
fn read_package_hook_without_a_checksum_resolves_every_dependency_again() {
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let options = hooked_install_options(temp_dir.path(), &[FOO], None);
    install_with_hook(&options);
    let lockfile = read_lockfile(&options);
    assert!(lockfile.contains("untrackedPnpmfileReadPackageHook: true"));
    assert!(!lockfile.contains("pnpmfileChecksum"));

    let options = hooked_install_options(temp_dir.path(), &[FOO, BAR], None);
    assert!(
        install_with_hook(&options)
            .iter()
            .any(|name| name == FOO),
        "an untracked hook cannot vouch for the lockfile",
    );
}

#[test]
fn adding_an_untracked_read_package_hook_resolves_again() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let options = hooked_install_options(temp_dir.path(), &[FOO], None);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("install without a hook");
    assert!(!read_lockfile(&options).contains("untrackedPnpmfileReadPackageHook"));

    assert!(
        install_with_hook(&options)
            .iter()
            .any(|name| name == FOO),
        "a hook the lockfile does not record has to be applied",
    );
    assert!(read_lockfile(&options).contains("untrackedPnpmfileReadPackageHook: true"));
}

#[test]
fn rebuild_accepts_a_lockfile_resolved_with_other_hooks_and_settings() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let options = hooked_install_options(temp_dir.path(), &[FOO], Some("hooks-1"));
    install_with_hook(&options);
    assert!(read_lockfile(&options).contains("pnpmfileChecksum: hooks-1"));

    for checksum in [Some("hooks-1"), Some("hooks-2"), None] {
        let options = hooked_install_options(temp_dir.path(), &[FOO], checksum);
        run_install_inner(&options, None, EngineMode::Rebuild(rebuild_options(None)))
            .unwrap_or_else(|error| panic!("rebuild with checksum {checksum:?}: {error}"));
    }

    let mut options = hooked_install_options(temp_dir.path(), &[FOO], Some("hooks-1"));
    // An override of a direct dependency also rewrites its specifier in the
    // manifest check, which has to apply the overrides the lockfile records.
    options.overrides = Some(IndexMap::from([(FOO.to_string(), "100.1.0".to_string())]));
    run_install_inner(&options, None, EngineMode::Rebuild(rebuild_options(None)))
        .expect("rebuild with an override the lockfile does not record");

    assert!(
        read_lockfile(&options).contains("pnpmfileChecksum: hooks-1"),
        "a rebuild leaves the lockfile alone",
    );
}

#[test]
fn rebuild_still_checks_the_manifests_against_the_lockfile() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let options = hooked_install_options(temp_dir.path(), &[FOO], Some("hooks-1"));
    install_with_hook(&options);

    // A rebuild materializes the lockfile, so it must not fetch and build
    // a dependency the manifest no longer declares.
    let options = hooked_install_options(temp_dir.path(), &[], Some("hooks-1"));
    let error = run_install_inner(&options, None, EngineMode::Rebuild(rebuild_options(None)))
        .expect_err("a rebuild of a lockfile the manifest no longer matches");
    eprintln!("{error}");
    assert!(error.to_string().contains("ERR_PNPM_OUTDATED_LOCKFILE"));
}
