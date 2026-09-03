#[cfg(windows)]
use super::validate_candidate;
use super::{
    Candidate, apply_state_dir_setting, find_candidate,
    identity::{
        MAX_HASHED_BIN_SIZE, local_bin_identity, package_dir_of_target, provider_of_target,
        read_shim_target_from_content, small_file_hash,
    },
    is_automatic_runtime, local_bin_path, local_bin_unchanged, manifest_runtime_pin,
    runtime_env::managed_runtime_bin,
    trust::{append_trust_decision, read_trust_decision},
    try_dispatch,
};
use pnpm_config::ShimPolicy;
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
fn malformed_legacy_shim_argv_fails_instead_of_running_the_cli() {
    assert_eq!(try_dispatch(&strings(&["pnpm", "--shim"])), Some(1));
    assert_eq!(try_dispatch(&strings(&["pnpm", "--shim", "tool", "/g/bin/tool"])), Some(1));
    assert_eq!(
        try_dispatch(&strings(&["pnpm", "--shim", "tool", "/g/bin/tool", "/g/pkg/cli", "x"])),
        Some(1),
    );
    assert_eq!(
        try_dispatch(&strings(&["pnpm", "--shim", "../tool", "/g/bin/tool", "/g/pkg/cli", "--"])),
        Some(1),
    );
    assert_eq!(
        try_dispatch(&strings(&["pnpm", "--shim", "tool", "/g/bin/tool", "pkg:not valid", "--"])),
        Some(1),
    );
}

#[test]
fn configured_state_dir_resolves_relative_to_the_machine_state_root() {
    let root = tempfile::tempdir().unwrap();
    let default_state_dir = root.path().join("state/pnpm");
    let expected_state_dir = dunce::canonicalize(root.path()).unwrap().join("state/custom-state");
    let mut state_dir = default_state_dir.clone();
    apply_state_dir_setting(&mut state_dir, Some("custom-state"), &default_state_dir);
    assert_eq!(state_dir, expected_state_dir);

    apply_state_dir_setting(&mut state_dir, Some(""), &default_state_dir);
    assert_eq!(state_dir, expected_state_dir);

    let absolute_state_dir = root.path().join("absolute-state");
    apply_state_dir_setting(&mut state_dir, absolute_state_dir.to_str(), &default_state_dir);
    assert_eq!(state_dir, absolute_state_dir);

    apply_state_dir_setting(&mut state_dir, Some("relative"), Path::new(""));
    assert!(state_dir.as_os_str().is_empty());

    state_dir = default_state_dir.clone();
    apply_state_dir_setting(&mut state_dir, Some("../outside"), &default_state_dir);
    assert!(state_dir.as_os_str().is_empty());
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

    let candidate = find_candidate(&nested, "tsc", "typescript").unwrap();
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
    assert!(find_candidate(&dir, "tsc", "typescript").is_none());
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

    let candidate = find_candidate(&nested, "node", "node").unwrap();
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

    assert!(find_candidate(root.path(), "node", "node").is_none());
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

#[cfg(windows)]
#[test]
fn windows_cmd_shim_candidate_matches_the_global_provider() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let local_package = project.join("node_modules/tool");
    let local_target = local_package.join("cli.cmd");
    let local_bin = project.join("node_modules/.bin");
    let global_package = root.path().join("global/node_modules/tool");
    let global_target = global_package.join("cli.cmd");
    fs::create_dir_all(&local_package).unwrap();
    fs::create_dir_all(&local_bin).unwrap();
    fs::create_dir_all(&global_package).unwrap();
    fs::write(local_package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(global_package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(&local_target, "@ECHO local:%*\r\n").unwrap();
    fs::write(&global_target, "@ECHO global:%*\r\n").unwrap();
    fs::write(local_bin.join("tool"), format!("# cmd-shim-target={}\n", local_target.display()))
        .unwrap();
    fs::write(local_bin.join("tool.cmd"), format!("@CALL \"{}\" %*\r\n", local_target.display()))
        .unwrap();

    let global_provider = provider_of_target(&global_target).unwrap();
    assert_eq!(global_provider.name, "tool");
    let candidate = find_candidate(&project, "tool", "tool").unwrap();
    let Candidate::LocalBin { bin, .. } = &candidate else {
        panic!("expected a local bin candidate");
    };
    assert_eq!(local_bin_identity(bin, "tool").unwrap().provider.name, "tool");
    assert!(validate_candidate(candidate, &global_provider.name, "tool").is_some());
}

/// The Windows dispatch flavor (`tool.cmd`) and the trailer-carrying sh
/// flavor (`tool`) are different files; the fingerprint must change when
/// either does.
#[test]
fn local_bin_fingerprint_binds_the_executed_flavor() {
    let root = tempfile::tempdir().unwrap();
    let modules = root.path().join("node_modules");
    let bin_dir = modules.join(".bin");
    fs::create_dir_all(modules.join("tool")).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(modules.join("tool").join("cli.js"), "").unwrap();
    fs::write(modules.join("tool").join("package.json"), r#"{"name":"tool","version":"1.0.0"}"#)
        .unwrap();
    fs::write(
        bin_dir.join("tool"),
        format!(
            "#!/bin/sh\nexec x\n# cmd-shim-target={}\n",
            modules.join("tool").join("cli.js").display(),
        ),
    )
    .unwrap();
    let cmd_flavor = bin_dir.join("tool.cmd");
    fs::write(&cmd_flavor, "@ECHO original\r\n").unwrap();

    let before = local_bin_identity(&cmd_flavor, "tool").unwrap().fingerprint;
    fs::write(&cmd_flavor, "@ECHO replaced\r\n").unwrap();
    let after = local_bin_identity(&cmd_flavor, "tool").unwrap().fingerprint;
    assert_ne!(before, after, "replacing the executed flavor must invalidate the approval");
}

#[test]
fn local_bin_identity_rejects_an_oversized_executed_flavor() {
    let root = tempfile::tempdir().unwrap();
    let modules = root.path().join("node_modules");
    let bin_dir = modules.join(".bin");
    let package = modules.join("tool");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("cli.js"), "").unwrap();
    fs::write(package.join("package.json"), r#"{"name":"tool","version":"1.0.0"}"#).unwrap();
    fs::write(
        bin_dir.join("tool"),
        format!("# cmd-shim-target={}\n", package.join("cli.js").display()),
    )
    .unwrap();
    let cmd_flavor = bin_dir.join("tool.cmd");
    fs::write(&cmd_flavor, vec![b'x'; MAX_HASHED_BIN_SIZE as usize + 1]).unwrap();

    assert!(local_bin_identity(&cmd_flavor, "tool").is_none());
}

/// The window between the trust prompt and execution must not accept a
/// swapped bin: revalidation compares the live fingerprint against the
/// one the decision was made for.
#[test]
fn revalidation_rejects_a_bin_swapped_after_approval() {
    let root = tempfile::tempdir().unwrap();
    let modules = root.path().join("node_modules");
    let bin_dir = modules.join(".bin");
    fs::create_dir_all(modules.join("tool")).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(modules.join("tool").join("cli.js"), "").unwrap();
    fs::write(modules.join("tool").join("package.json"), r#"{"name":"tool","version":"1.0.0"}"#)
        .unwrap();
    let bin = bin_dir.join("tool");
    fs::write(
        &bin,
        format!(
            "#!/bin/sh\nexec x\n# cmd-shim-target={}\n",
            modules.join("tool").join("cli.js").display(),
        ),
    )
    .unwrap();

    let approved = local_bin_identity(&bin, "tool").unwrap().fingerprint;
    assert!(local_bin_unchanged(&bin, "tool", &approved));

    fs::write(
        &bin,
        format!(
            "#!/bin/sh\nexec swapped\n# cmd-shim-target={}\n",
            modules.join("tool").join("cli.js").display(),
        ),
    )
    .unwrap();
    assert!(!local_bin_unchanged(&bin, "tool", &approved));
    fs::remove_file(&bin).unwrap();
    assert!(!local_bin_unchanged(&bin, "tool", &approved));
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

#[test]
fn runtime_promptless_policy_matrix() {
    use super::runtime_runs_promptless;
    // Auto defers to artifact authentication: stable release yes, rc no.
    assert!(runtime_runs_promptless(ShimPolicy::Auto, "node", "22.11.0"));
    assert!(!runtime_runs_promptless(ShimPolicy::Auto, "node", "24.0.0-rc.4"));
    assert!(!runtime_runs_promptless(ShimPolicy::Auto, "deno", "2.0.0"));
    // Always always may; Prompt and Off never.
    assert!(runtime_runs_promptless(ShimPolicy::Always, "deno", "2.0.0"));
    assert!(runtime_runs_promptless(ShimPolicy::Always, "node", "24.0.0-rc.4"));
    assert!(!runtime_runs_promptless(ShimPolicy::Prompt, "node", "22.11.0"));
    assert!(!runtime_runs_promptless(ShimPolicy::Off, "node", "22.11.0"));
}
