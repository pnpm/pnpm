use super::{
    EngineMode, HashMap, NodeApiProject, TestRegistry, install_options, run_install_inner,
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
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));
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
