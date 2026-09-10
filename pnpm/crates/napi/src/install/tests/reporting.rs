use super::{
    Arc, BTreeSet, DepsRequiringBuildSink, EngineMode, HashMap, NodeApiProject, TestRegistry,
    WELL_FORMED_PATCH, install_options, install_options_for, run_install_inner,
    script_deps_install_options, take_deps_requiring_build,
};

/// The result preserves the sink's order so a consumer diffing it against
/// a recorded list sees no spurious churn.
#[test]
fn take_deps_requiring_build_reports_the_list_in_sorted_order() {
    let sink = DepsRequiringBuildSink::default();
    *sink.lock().expect("lock sink") = Some(BTreeSet::from([
        "zzz@1.0.0".to_string(),
        "aaa@1.0.0".to_string(),
        "mmm@1.0.0".to_string(),
    ]));

    assert_eq!(
        take_deps_requiring_build(Some(&sink), Vec::new()),
        Some(vec!["aaa@1.0.0".to_string(), "mmm@1.0.0".to_string(), "zzz@1.0.0".to_string()]),
    );
}

/// `returnListOfDepsRequiringBuild` reports every package whose files
/// carry install scripts, sorted. `hello-world-js-bin` arrives as a
/// dependency of the postinstall example and carries no install scripts
/// of its own, so it must not appear.
#[test]
fn return_list_of_deps_requiring_build_reports_every_script_bearing_package() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = script_deps_install_options(temp_dir.path());
    options.dangerously_allow_all_builds = Some(true);

    let sink = DepsRequiringBuildSink::default();
    run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&sink))))
        .expect("install");

    assert_eq!(
        take_deps_requiring_build(Some(&sink), Vec::new()),
        Some(vec![
            "@pnpm.e2e/install-script-example@1.0.0".to_string(),
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0".to_string(),
        ]),
    );
}

/// A tree with no script-bearing package has nothing to build, and that
/// is an answer. The install reports an empty list rather than none, so
/// an embedder replaces its recorded list instead of keeping a stale one.
#[test]
fn return_list_of_deps_requiring_build_reports_an_empty_list_for_a_tree_without_build_scripts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let options = install_options_for(
        temp_dir.path(),
        "project",
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );

    let sink = DepsRequiringBuildSink::default();
    run_install_inner(&options, None, EngineMode::Install(Some(Arc::clone(&sink))))
        .expect("install");

    assert_eq!(take_deps_requiring_build(Some(&sink), Vec::new()), Some(Vec::new()));
}

#[test]
fn allow_unused_patches_downgrades_an_unmatched_patch_to_a_warning() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");
    std::fs::create_dir(project_dir.join("patches")).expect("create patches dir");
    std::fs::write(project_dir.join("patches/unmatched.patch"), WELL_FORMED_PATCH)
        .expect("write patch file");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));
    options.patched_dependencies = Some(indexmap::IndexMap::from_iter([(
        "is-negative@1.0.0".to_string(),
        "patches/unmatched.patch".to_string(),
    )]));

    let error = run_install_inner(&options, None, EngineMode::Install(None))
        .expect_err("an unmatched patch must fail the install");
    assert!(
        error.reason.contains("ERR_PNPM_UNUSED_PATCH"),
        "expected ERR_PNPM_UNUSED_PATCH, got: {reason}",
        reason = error.reason,
    );

    options.allow_unused_patches = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("allowUnusedPatches must let the install through");
}
