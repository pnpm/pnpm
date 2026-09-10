use std::{fmt::Write as _, fs, path::Path};

/// Set `strictDepBuilds`. Tests that intentionally leave builds
/// ignored and then inspect the filesystem set it to `false` so the
/// install completes instead of failing with
/// `ERR_PNPM_IGNORED_BUILDS` (the default). Must be called before
/// [`allow_builds`] so its line survives the `allowBuilds:`
/// truncation on re-calls.
pub fn set_strict_dep_builds(workspace: &Path, strict: bool) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    writeln!(yaml, "strictDepBuilds: {strict}").expect("format strictDepBuilds");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// Append a top-level `key: value` line. Only valid for keys the
/// harness never writes; the assert fails loudly if that changes.
pub fn append_workspace_yaml_key(workspace: &Path, key: &str, value: impl std::fmt::Display) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
    let key_prefix = format!("{key}:");
    assert!(
        !yaml.lines().any(|line| line.starts_with(&key_prefix)),
        "pnpm-workspace.yaml already has a `{key}:` key",
    );
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    writeln!(yaml, "{key}: {value}").expect("format workspace key");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// Set the `allowBuilds:` block, replacing any block a previous
/// phase wrote. pnpm v11 (and pacquet) read build approval from
/// `pnpm-workspace.yaml`, not from `package.json#pnpm` — this
/// mirrors upstream tests passing `allowBuilds` through
/// `testDefaults`. An empty `entries` withdraws every approval.
pub fn allow_builds(workspace: &Path, entries: &[(&str, bool)]) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
    if let Some(idx) = yaml.find("allowBuilds:") {
        yaml.truncate(idx);
    }
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    if entries.is_empty() {
        fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
        return;
    }
    yaml.push_str("allowBuilds:\n");
    for (spec, value) in entries {
        writeln!(yaml, "  '{spec}': {value}").expect("format allowBuilds entry");
    }
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}
