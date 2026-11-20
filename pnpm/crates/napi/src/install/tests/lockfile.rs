use super::{
    EngineMode,
    HashMap,
    NodeApiProject,
    TestRegistry,
    install_options,
    install_options_for,
    run_install_inner,
};

/// The lockfile must record `overrides` in declaration order — the
/// napi boundary carries them through an `IndexMap`, and a `HashMap`
/// regression would rewrite the block in a random order on every
/// install (a sorted map would flip the non-lexicographic order below).
#[test]
fn lockfile_records_overrides_in_declaration_order() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0"
            }
        }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(
        temp_dir
            .path()
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));
    options.overrides = Some(indexmap::IndexMap::from_iter([
        ("zzz-unmatched".to_string(), "1.0.0".to_string()),
        ("aaa-unmatched".to_string(), "2.0.0".to_string()),
    ]));

    run_install_inner(&options, None, EngineMode::Install(None)).expect("install");

    let lockfile =
        std::fs::read_to_string(project_dir.join("pnpm-lock.yaml")).expect("read lockfile");
    let zzz = lockfile.find("zzz-unmatched").expect("zzz override recorded");
    let aaa = lockfile.find("aaa-unmatched").expect("aaa override recorded");
    assert!(zzz < aaa, "overrides must keep declaration order (zzz before aaa), got:\n{lockfile}");
}

#[test]
fn lockfile_format_selects_the_written_lockfile_version() {
    let registry = TestRegistry::start();
    for (lockfile_format, expected_version) in [(Some("v13"), "13.0"), (None, "9.0")] {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let project_dir = temp_dir.path().join("project");
        std::fs::create_dir(&project_dir).expect("create project dir");
        std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

        let project_dir_string = project_dir.to_string_lossy().into_owned();
        let mut options = install_options();
        options.dir = project_dir_string.clone();
        options.projects = vec![NodeApiProject {
            root_dir: project_dir_string,
            manifest: serde_json::json!({
                "dependencies": {
                    "@pnpm.e2e/foo": "100.0.0"
                }
            }),
            dependency_manifest: None,
        }];
        options.store_dir = Some(
            temp_dir
                .path()
                .join("store")
                .to_string_lossy()
                .into_owned(),
        );
        options.registries =
            Some(HashMap::from([("default".to_string(), registry.url().to_string())]));
        options.lockfile_format = lockfile_format.map(str::to_string);

        run_install_inner(&options, None, EngineMode::Install(None)).expect("install");

        let lockfile =
            std::fs::read_to_string(project_dir.join("pnpm-lock.yaml")).expect("read lockfile");
        assert_eq!(
            lockfile_version(&lockfile),
            Some(expected_version),
            "lockfileFormat {lockfile_format:?} must write lockfileVersion {expected_version}, got:\n{lockfile}",
        );
    }
}

/// Removing a dependency is absorbed by the fast lockfile update, which
/// writes the loaded lockfile instead of a freshly resolved one.
#[test]
fn lockfile_format_applies_to_a_fast_updated_lockfile() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let project = |manifest: serde_json::Value| NodeApiProject {
        root_dir: project_dir_string.clone(),
        manifest,
        dependency_manifest: None,
    };
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![project(serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/bar": "100.0.0"
        }
    }))];
    options.store_dir = Some(
        temp_dir
            .path()
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));
    run_install_inner(&options, None, EngineMode::Install(None)).expect("install with v9");

    options.projects = vec![project(serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/foo": "100.0.0"
        }
    }))];
    options.lockfile_format = Some("v13".to_string());
    run_install_inner(&options, None, EngineMode::Install(None)).expect("install with v13");

    let lockfile =
        std::fs::read_to_string(project_dir.join("pnpm-lock.yaml")).expect("read lockfile");
    assert_eq!(lockfile_version(&lockfile), Some("13.0"), "got:\n{lockfile}");
    assert!(
        !lockfile.contains("@pnpm.e2e/bar"),
        "the removed dependency is gone, got:\n{lockfile}",
    );
}

/// The `lockfileVersion` of a lockfile in either the YAML (`9.0`) or the
/// JSON-shaped (`13.0`) format.
fn lockfile_version(lockfile: &str) -> Option<&str> {
    lockfile
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("lockfileVersion: ")
                .or_else(|| line.trim().strip_prefix(r#""lockfileVersion": "#))
        })
        .map(|value| {
            value
                .trim_end_matches(',')
                .trim_matches(['\'', '"'])
        })
}

#[test]
fn ignore_lockfile_settings_checks_keeps_a_lockfile_with_mismatched_settings() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = install_options_for(
        temp_dir.path(),
        "project",
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    let lockfile_path = temp_dir.path().join("project/pnpm-lock.yaml");
    options.auto_install_peers = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("seed install");
    let seeded = std::fs::read_to_string(&lockfile_path).expect("read lockfile");

    options.auto_install_peers = Some(false);
    options.frozen_lockfile = Some(true);
    let error = run_install_inner(&options, None, EngineMode::Install(None))
        .expect_err("a frozen install rejects the mismatched settings");
    eprintln!("{error}");
    assert!(error.reason.contains("ERR_PNPM_LOCKFILE_CONFIG_MISMATCH"));

    options.ignore_lockfile_settings_checks = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("a frozen install ignoring the settings check");

    options.frozen_lockfile = None;
    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("an install ignoring the settings check");
    assert_eq!(
        std::fs::read_to_string(&lockfile_path).expect("read lockfile"),
        seeded,
        "the install must reuse the lockfile instead of resolving again",
    );
}
