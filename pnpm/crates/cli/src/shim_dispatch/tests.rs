use super::{
    Candidate, MAX_HASHED_BIN_SIZE, append_trust_decision, find_candidate, install_dispatcher_from,
    is_automatic_runtime, local_bin_identity, local_bin_path, managed_runtime_bin,
    manifest_runtime_pin, package_dir_of_target, parse_shim_argv, provider_of_target,
    read_shim_target_from_content, read_trust_decision, small_file_hash, try_dispatch,
};
use pacquet_config::GlobalShims;
use std::{ffi::OsString, fs, path::Path};

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn non_shim_argv_is_not_intercepted() {
    assert!(try_dispatch(&strings(&["pnpm"])).is_none());
    assert!(try_dispatch(&strings(&["pnpm", "install"])).is_none());
    assert!(try_dispatch(&strings(&["pnpm", "add", "--shim"])).is_none());
}

#[test]
fn parses_the_generated_shim_argv() {
    let rest = strings(&["node", "/global/bin/node", "/global/node", "--", "--version", "-e", "1"]);
    let (name, shim, target, args) = parse_shim_argv(&rest).unwrap();
    assert_eq!(name, "node");
    assert_eq!(shim, Path::new("/global/bin/node"));
    assert_eq!(target, Path::new("/global/node"));
    assert_eq!(args, &strings(&["--version", "-e", "1"])[..]);
}

#[test]
fn rejects_malformed_shim_argv() {
    assert!(parse_shim_argv(&strings(&[])).is_none());
    assert!(parse_shim_argv(&strings(&["node"])).is_none());
    assert!(parse_shim_argv(&strings(&["node", "/t"])).is_none());
    assert!(parse_shim_argv(&strings(&["node", "/s", "/t", "--version"])).is_none());
}

#[test]
fn empty_args_after_separator_parse() {
    let rest = strings(&["tsc", "/s", "/t", "--"]);
    let (name, _, _, args) = parse_shim_argv(&rest).unwrap();
    assert_eq!(name, "tsc");
    assert!(args.is_empty());
}

#[test]
fn finds_the_nearest_local_bin_walking_up() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let nested = project.join("packages").join("app");
    let bin_dir = project.join("node_modules").join(".bin");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("tsc"), "#!/bin/sh\n").unwrap();

    let candidate = find_candidate(&nested, "tsc", GlobalShims::All).unwrap();
    let Candidate::LocalBin { project_dir, bin, .. } = candidate else {
        panic!("expected a local bin candidate");
    };
    assert_eq!(project_dir, project);
    assert_eq!(bin, bin_dir.join("tsc"));
}

#[test]
fn no_candidate_without_a_bin_or_pin() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("plain");
    fs::create_dir_all(&dir).unwrap();
    assert!(find_candidate(&dir, "tsc", GlobalShims::All).is_none());
}

#[test]
fn auto_mode_does_not_consider_ordinary_local_bins() {
    let root = tempfile::tempdir().unwrap();
    let bin_dir = root.path().join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("tsc"), "#!/bin/sh\n").unwrap();

    assert!(find_candidate(root.path(), "tsc", GlobalShims::Auto).is_none());
}

#[test]
fn local_bin_ignores_directories() {
    let root = tempfile::tempdir().unwrap();
    let bin_dir = root.path().join("node_modules").join(".bin");
    fs::create_dir_all(bin_dir.join("tsc")).unwrap();
    assert!(local_bin_path(root.path(), "tsc").is_none());
}

#[test]
fn runtime_pin_prefers_dev_engines_and_supports_arrays() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        serde_json::json!({
            "devEngines": {
                "runtime": [
                    { "name": "deno", "version": "2.0.0" },
                    { "name": "node", "version": "22.11.0", "onFail": "download" },
                ],
            },
            "engines": { "runtime": { "name": "node", "version": "20.0.0" } },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        manifest_runtime_pin(root.path(), "node").map(|pin| pin.0).as_deref(),
        Some("22.11.0"),
    );
    assert_eq!(
        manifest_runtime_pin(root.path(), "deno").map(|pin| pin.0).as_deref(),
        Some("2.0.0"),
    );
    assert_eq!(manifest_runtime_pin(root.path(), "bun"), None);
}

#[test]
fn runtime_pin_falls_back_to_engines() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        serde_json::json!({
            "engines": { "runtime": { "name": "node", "version": "20.1.0" } },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        manifest_runtime_pin(root.path(), "node").map(|pin| pin.0).as_deref(),
        Some("20.1.0"),
    );
}

#[test]
fn runtime_pin_candidate_found_walking_up() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let nested = project.join("src");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        project.join("package.json"),
        serde_json::json!({
            "devEngines": { "runtime": { "name": "node", "version": "22.0.0" } },
        })
        .to_string(),
    )
    .unwrap();

    let candidate = find_candidate(&nested, "node", GlobalShims::Auto).unwrap();
    let Candidate::RuntimePin { project_dir, version_spec, .. } = candidate else {
        panic!("expected a runtime pin candidate");
    };
    assert_eq!(project_dir, project);
    assert_eq!(version_spec, "22.0.0");
}

#[test]
fn runtime_candidates_never_use_project_bin_entries() {
    let root = tempfile::tempdir().unwrap();
    let bin_dir = root.path().join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("node"), "compromised").unwrap();

    assert!(find_candidate(root.path(), "node", GlobalShims::All).is_none());
}

#[test]
fn auto_mode_only_accepts_signed_node_release_channel() {
    assert!(is_automatic_runtime("node", "22.11.0"));
    assert!(is_automatic_runtime("node", "22"));
    assert!(!is_automatic_runtime("node", "rc/24"));
    assert!(!is_automatic_runtime("node", "24.0.0-rc.4"));
    assert!(!is_automatic_runtime("deno", "2.0.0"));
    assert!(!is_automatic_runtime("bun", "1.2.0"));
}

#[cfg(unix)]
#[test]
fn managed_runtime_must_resolve_inside_the_global_store() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("store/links");
    let package = store.join("node/22/slot/node_modules/node");
    let environment_modules = root.path().join("state/environment/node_modules");
    fs::create_dir_all(package.join("bin")).unwrap();
    fs::create_dir_all(&environment_modules).unwrap();
    fs::write(package.join("package.json"), r#"{"name":"node","bin":{"node":"bin/node"}}"#)
        .unwrap();
    fs::write(package.join("bin/node"), "runtime").unwrap();
    std::os::unix::fs::symlink(&package, environment_modules.join("node")).unwrap();

    assert_eq!(
        managed_runtime_bin(root.path().join("state/environment").as_path(), "node", &store),
        Some(fs::canonicalize(package.join("bin/node")).unwrap()),
    );

    fs::remove_file(environment_modules.join("node")).unwrap();
    let outside = root.path().join("outside/node");
    fs::create_dir_all(outside.join("bin")).unwrap();
    fs::write(outside.join("package.json"), r#"{"name":"node","bin":{"node":"bin/node"}}"#)
        .unwrap();
    fs::write(outside.join("bin/node"), "runtime").unwrap();
    std::os::unix::fs::symlink(outside, environment_modules.join("node")).unwrap();
    assert_eq!(
        managed_runtime_bin(root.path().join("state/environment").as_path(), "node", &store),
        None,
    );
}

#[test]
fn trust_decisions_round_trip_last_record_wins() {
    let root = tempfile::tempdir().unwrap();
    let trust_file = root.path().join("state").join("global-bin-trust.jsonl");

    assert_eq!(read_trust_decision(&trust_file, "/a", "candidate-a"), None);
    append_trust_decision(&trust_file, "/a", "candidate-a", true).unwrap();
    append_trust_decision(&trust_file, "/b", "candidate-b", false).unwrap();
    assert_eq!(read_trust_decision(&trust_file, "/a", "candidate-a"), Some(true));
    assert_eq!(read_trust_decision(&trust_file, "/b", "candidate-b"), Some(false));
    assert_eq!(read_trust_decision(&trust_file, "/c", "candidate-c"), None);
    assert_eq!(read_trust_decision(&trust_file, "/a", "changed"), None);

    append_trust_decision(&trust_file, "/a", "candidate-a", false).unwrap();
    assert_eq!(read_trust_decision(&trust_file, "/a", "candidate-a"), Some(false));
}

#[test]
fn trust_registry_tolerates_corrupt_lines() {
    let root = tempfile::tempdir().unwrap();
    let trust_file = root.path().join("global-bin-trust.jsonl");
    fs::write(
        &trust_file,
        "not json\n{\"projectDir\":\"/a\",\"candidateId\":\"candidate-a\",\"allow\":true}\n",
    )
    .unwrap();
    assert_eq!(read_trust_decision(&trust_file, "/a", "candidate-a"), Some(true));
}

#[test]
fn legacy_path_only_trust_records_are_ignored() {
    let root = tempfile::tempdir().unwrap();
    let trust_file = root.path().join("global-bin-trust.jsonl");
    fs::write(&trust_file, r#"{"projectDir":"/a","allow":true}"#).unwrap();
    assert_eq!(read_trust_decision(&trust_file, "/a", "candidate-a"), None);
}

#[test]
fn package_root_is_the_nearest_manifest_ancestor() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("workspace-package");
    let nested_target = package.join("dist/bin/cli.js");
    fs::create_dir_all(nested_target.parent().unwrap()).unwrap();
    fs::write(package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(&nested_target, "").unwrap();
    assert_eq!(package_dir_of_target(&nested_target), Some(package));
    assert_eq!(package_dir_of_target(root.path()), None);
}

#[test]
fn versioned_dispatcher_survives_main_executable_replacement() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("pnpm");
    let destination = root.path().join(".pnpm-shim-v1");
    fs::write(&source, "v12 dispatcher").unwrap();
    install_dispatcher_from(&source, &destination).unwrap();

    fs::rename(&source, root.path().join("pnpm-v12")).unwrap();
    fs::write(&source, "pre-v12 executable").unwrap();

    assert_eq!(fs::read_to_string(destination).unwrap(), "v12 dispatcher");
}

#[test]
fn dispatcher_install_replaces_a_stale_file() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("pnpm");
    let destination = root.path().join(".pnpm-shim-v1");
    fs::write(&source, "current dispatcher").unwrap();
    fs::write(&destination, "stale dispatcher").unwrap();

    install_dispatcher_from(&source, &destination).unwrap();

    assert_eq!(fs::read_to_string(destination).unwrap(), "current dispatcher");
}

#[test]
fn small_bin_hash_is_content_bound_and_bounded() {
    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), b"aaaa").unwrap();
    let before = small_file_hash(file.path(), 4).unwrap();
    fs::write(file.path(), b"bbbb").unwrap();
    let after = small_file_hash(file.path(), 4).unwrap();

    assert_ne!(before, after);
    assert_eq!(small_file_hash(file.path(), MAX_HASHED_BIN_SIZE + 1), None);
}

#[test]
fn shim_target_trailer_round_trips() {
    let with_target = "#!/bin/sh\nexec something\n# pnpm-shim-style=context-aware\n# cmd-shim-target=/g/node_modules/tool/cli.js\n";
    assert_eq!(
        read_shim_target_from_content(with_target),
        Some("/g/node_modules/tool/cli.js".into()),
    );
    assert_eq!(read_shim_target_from_content("#!/bin/sh\nexec something\n"), None);
}

#[cfg(unix)]
#[test]
fn local_bin_identity_resolves_symlinks_and_trailers() {
    let root = tempfile::tempdir().unwrap();
    let modules = root.path().join("node_modules");
    let bin_dir = modules.join(".bin");
    fs::create_dir_all(modules.join("tool")).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(modules.join("tool").join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(modules.join("tool").join("cli.js"), "").unwrap();

    let linked = bin_dir.join("linked");
    std::os::unix::fs::symlink("../tool/cli.js", &linked).unwrap();
    assert_eq!(local_bin_identity(&linked, "linked").unwrap().provider.name, "tool");

    let scripted = bin_dir.join("scripted");
    fs::write(
        &scripted,
        format!(
            "#!/bin/sh\nexec x\n# cmd-shim-target={}\n",
            modules.join("tool").join("cli.js").display(),
        ),
    )
    .unwrap();
    assert_eq!(local_bin_identity(&scripted, "scripted").unwrap().provider.name, "tool");

    let bare = bin_dir.join("bare");
    fs::write(&bare, "#!/bin/sh\n").unwrap();
    assert!(local_bin_identity(&bare, "bare").is_none());
}

#[cfg(unix)]
#[test]
fn local_bin_identity_changes_with_the_project_lockfile() {
    let root = tempfile::tempdir().unwrap();
    let modules = root.path().join("node_modules");
    let bin_dir = modules.join(".bin");
    let package = modules.join("tool");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(package.join("cli.js"), "").unwrap();
    let bin = bin_dir.join("tool");
    std::os::unix::fs::symlink("../tool/cli.js", &bin).unwrap();

    fs::write(root.path().join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let before = local_bin_identity(&bin, "tool").unwrap().fingerprint;
    fs::write(
        root.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: true\n",
    )
    .unwrap();
    let after = local_bin_identity(&bin, "tool").unwrap().fingerprint;

    assert_ne!(before, after);
}

#[cfg(unix)]
#[test]
fn provider_identity_follows_aliases_to_the_actual_manifest() {
    let root = tempfile::tempdir().unwrap();
    let modules = root.path().join("node_modules");
    let actual = modules.join(".pnpm/evil@1.0.0/node_modules/evil");
    fs::create_dir_all(&actual).unwrap();
    fs::write(actual.join("package.json"), r#"{"name":"evil","version":"1.0.0"}"#).unwrap();
    fs::write(actual.join("cli.js"), "").unwrap();
    std::os::unix::fs::symlink(&actual, modules.join("tool")).unwrap();

    let provider = provider_of_target(&modules.join("tool/cli.js")).unwrap();
    assert_eq!(provider.name, "evil");
}

#[cfg(unix)]
#[test]
fn provider_identity_supports_workspace_packages() {
    let root = tempfile::tempdir().unwrap();
    let workspace_package = root.path().join("packages/tool");
    let modules = root.path().join("node_modules");
    fs::create_dir_all(&workspace_package).unwrap();
    fs::create_dir_all(&modules).unwrap();
    fs::write(workspace_package.join("package.json"), r#"{"name":"tool","version":"1.0.0"}"#)
        .unwrap();
    fs::write(workspace_package.join("cli.js"), "").unwrap();
    std::os::unix::fs::symlink(&workspace_package, modules.join("tool")).unwrap();

    let provider = provider_of_target(&modules.join("tool/cli.js")).unwrap();
    assert_eq!(provider.name, "tool");
    assert_eq!(provider.package_dir, fs::canonicalize(workspace_package).unwrap());
}
