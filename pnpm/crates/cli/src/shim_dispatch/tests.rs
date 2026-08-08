use super::{
    Candidate, append_trust_decision, find_candidate, local_bin_path, manifest_runtime_pin,
    parse_shim_argv, read_trust_decision, try_dispatch,
};
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
    let rest = strings(&["node", "/global/node", "--", "--version", "-e", "1"]);
    let (name, target, args) = parse_shim_argv(&rest).unwrap();
    assert_eq!(name, "node");
    assert_eq!(target, Path::new("/global/node"));
    assert_eq!(args, &strings(&["--version", "-e", "1"])[..]);
}

#[test]
fn rejects_malformed_shim_argv() {
    assert!(parse_shim_argv(&strings(&[])).is_none());
    assert!(parse_shim_argv(&strings(&["node"])).is_none());
    assert!(parse_shim_argv(&strings(&["node", "/t"])).is_none());
    assert!(parse_shim_argv(&strings(&["node", "/t", "--version"])).is_none());
}

#[test]
fn empty_args_after_separator_parse() {
    let rest = strings(&["tsc", "/t", "--"]);
    let (name, _, args) = parse_shim_argv(&rest).unwrap();
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

    let candidate = find_candidate(&nested, "tsc").unwrap();
    let Candidate::LocalBin { project_dir, bin } = candidate else {
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
    assert!(find_candidate(&dir, "tsc").is_none());
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
    assert_eq!(manifest_runtime_pin(root.path(), "node").as_deref(), Some("22.11.0"));
    assert_eq!(manifest_runtime_pin(root.path(), "deno").as_deref(), Some("2.0.0"));
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
    assert_eq!(manifest_runtime_pin(root.path(), "node").as_deref(), Some("20.1.0"));
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

    let candidate = find_candidate(&nested, "node").unwrap();
    let Candidate::RuntimePin { project_dir, version_spec } = candidate else {
        panic!("expected a runtime pin candidate");
    };
    assert_eq!(project_dir, project);
    assert_eq!(version_spec, "22.0.0");
}

#[test]
fn trust_decisions_round_trip_last_record_wins() {
    let root = tempfile::tempdir().unwrap();
    let trust_file = root.path().join("state").join("global-bin-trust.jsonl");

    assert_eq!(read_trust_decision(&trust_file, "/a"), None);
    append_trust_decision(&trust_file, "/a", true).unwrap();
    append_trust_decision(&trust_file, "/b", false).unwrap();
    assert_eq!(read_trust_decision(&trust_file, "/a"), Some(true));
    assert_eq!(read_trust_decision(&trust_file, "/b"), Some(false));
    assert_eq!(read_trust_decision(&trust_file, "/c"), None);

    append_trust_decision(&trust_file, "/a", false).unwrap();
    assert_eq!(read_trust_decision(&trust_file, "/a"), Some(false));
}

#[test]
fn trust_registry_tolerates_corrupt_lines() {
    let root = tempfile::tempdir().unwrap();
    let trust_file = root.path().join("global-bin-trust.jsonl");
    fs::write(&trust_file, "not json\n{\"projectDir\":\"/a\",\"allow\":true}\n").unwrap();
    assert_eq!(read_trust_decision(&trust_file, "/a"), Some(true));
}
