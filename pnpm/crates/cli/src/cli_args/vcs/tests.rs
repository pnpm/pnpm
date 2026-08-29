use super::{
    AutoSnappedComponent, BitSnapResult, BitStatusResult, assert_snap_protocol,
    assert_status_protocol, execute_bit, read_component_requirements, render_commit, render_status,
    sanitize_component_name, workspace_inventory,
};

use std::fs;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn renders_status() {
    let result = BitStatusResult {
        schema_version: 1,
        auto_tag_pending_components: Vec::new(),
        current_lane_id: Some("main".to_string()),
        modified_components: vec!["acme/parser".to_string()],
        new_components: vec!["acme/lexer".to_string()],
    };

    assert_eq!(
        render_status(&result),
        "Bit lane: main\n\nNew components:\n  acme/lexer\n\nModified components:\n  acme/parser",
    );
}

#[test]
fn renders_commit() {
    let result = BitSnapResult {
        schema_version: 1,
        snapped: true,
        batch_id: Some("batch-id".to_string()),
        lane_name: Some("feature".to_string()),
        snapped_components: vec!["acme/parser@abc123".to_string()],
        auto_snapped_components: vec![AutoSnappedComponent {
            id: "acme/app@def456".to_string(),
            triggered_by: vec!["acme/parser@abc123".to_string()],
        }],
        new_components: Vec::new(),
        removed_components: Vec::new(),
        warnings: Vec::new(),
        total_components_count: 2,
    };

    assert_snap_protocol(&result).expect("valid protocol");
    assert_eq!(
        render_commit(&result),
        "Created Bit snap batch batch-id on feature (2 components).\n  acme/parser@abc123\n  acme/app@def456",
    );
}

#[test]
fn rejects_unsupported_snap_protocol() {
    let result = BitSnapResult {
        schema_version: 0,
        snapped: true,
        batch_id: None,
        lane_name: None,
        snapped_components: Vec::new(),
        auto_snapped_components: Vec::new(),
        new_components: Vec::new(),
        removed_components: Vec::new(),
        warnings: Vec::new(),
        total_components_count: 0,
    };

    let message = assert_snap_protocol(&result).expect_err("unsupported protocol").to_string();
    assert_eq!(
        message,
        "The installed Bit version does not support pnpm VCS snap protocol version 1",
    );
}

#[test]
fn renders_clean_status() {
    let result = BitStatusResult {
        schema_version: 1,
        auto_tag_pending_components: Vec::new(),
        current_lane_id: None,
        modified_components: Vec::new(),
        new_components: Vec::new(),
    };

    assert_eq!(render_status(&result), "No component changes.");
}

#[test]
fn renders_clean_commit() {
    let result = BitSnapResult {
        schema_version: 1,
        snapped: false,
        batch_id: None,
        lane_name: None,
        snapped_components: Vec::new(),
        auto_snapped_components: Vec::new(),
        new_components: Vec::new(),
        removed_components: Vec::new(),
        warnings: Vec::new(),
        total_components_count: 0,
    };

    assert_snap_protocol(&result).expect("valid clean result");
    assert_eq!(render_commit(&result), "No component changes.");
}

#[test]
fn rejects_unsupported_status_protocol() {
    let result = BitStatusResult {
        schema_version: 0,
        auto_tag_pending_components: Vec::new(),
        current_lane_id: None,
        modified_components: Vec::new(),
        new_components: Vec::new(),
    };

    let message = assert_status_protocol(&result).expect_err("unsupported protocol").to_string();
    assert_eq!(
        message,
        "The installed Bit version does not support pnpm VCS status protocol version 1",
    );
}

#[cfg(unix)]
#[tokio::test]
async fn invokes_bit_directly_with_the_workspace_as_cwd() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let bit = fixture.path().join("bit");
    fs::write(&bit, "#!/bin/sh\nprintf '%s\\n%s' \"$PWD\" \"$*\"\n").expect("write fake Bit");
    fs::set_permissions(&bit, fs::Permissions::from_mode(0o755)).expect("make fake Bit executable");

    let stdout = execute_bit(
        bit.to_str().expect("UTF-8 fixture path"),
        &["snap", "--json", "--message", "workspace commit"],
        fixture.path(),
    )
    .await
    .expect("execute fake Bit");

    assert_eq!(
        stdout,
        format!("{}\nsnap --json --message workspace commit", fixture.path().display()),
    );
}

#[test]
fn builds_inventory_from_a_regular_pnpm_workspace() {
    let fixture = tempfile::tempdir().expect("create fixture");
    fs::create_dir_all(fixture.path().join("packages/foo")).expect("create package directory");
    fs::write(fixture.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    fs::write(fixture.path().join("package.json"), r#"{"name":"@acme/repository","private":true}"#)
        .expect("write root manifest");
    fs::write(
        fixture.path().join("packages/foo/package.json"),
        r#"{"name":"@acme/foo","version":"1.0.0"}"#,
    )
    .expect("write package manifest");
    let config = pnpm_config::Config {
        workspace_dir: Some(fixture.path().to_path_buf()),
        ..pnpm_config::Config::default()
    };

    let inventory = workspace_inventory(fixture.path(), &config, "acme.repository")
        .expect("build workspace inventory");

    assert_eq!(inventory.schema_version, 2);
    assert_eq!(inventory.default_scope, "acme.repository");
    assert_eq!(inventory.root_component_name, "acme/repository-workspace");
    assert!(inventory.workspace_profile.is_none());
    assert_eq!(inventory.projects.len(), 1);
    assert_eq!(inventory.projects[0].root_dir, "packages/foo");
    assert_eq!(inventory.projects[0].component_name, "acme/foo");
    assert!(inventory.projects[0].requirements.is_none());
}

#[test]
fn reads_a_locked_workspace_profile_and_component_requirements() {
    let fixture = tempfile::tempdir().expect("create fixture");
    fs::create_dir_all(fixture.path().join("packages/foo")).expect("create package directory");
    fs::write(fixture.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    fs::write(
        fixture.path().join("package.json"),
        r#"{
          "name":"@acme/repository",
          "private":true,
          "pnpm":{"vcs":{"profile":{
            "toolchain":{"implementation":"bit","version":"2.2.23"},
            "node":{"implementation":"node","version":"22.18.0"}
          }}}
        }"#,
    )
    .expect("write root manifest");
    fs::write(
        fixture.path().join("packages/foo/package.json"),
        r#"{
          "name":"@acme/foo",
          "version":"1.0.0",
          "engines":{"node":">=20 <23"},
          "pnpm":{"vcs":{"requirements":{
            "toolchain":{"implementation":"bit","version":"^2.2"}
          }}}
        }"#,
    )
    .expect("write package manifest");
    let config = pnpm_config::Config {
        workspace_dir: Some(fixture.path().to_path_buf()),
        ..pnpm_config::Config::default()
    };

    let inventory = workspace_inventory(fixture.path(), &config, "acme.repository")
        .expect("build workspace inventory");
    let profile = inventory.workspace_profile.expect("workspace profile");
    assert_eq!(profile["toolchain"].implementation, "bit");
    assert_eq!(profile["toolchain"].version, "2.2.23");
    let requirements = inventory.projects[0].requirements.as_ref().expect("component requirements");
    assert_eq!(requirements["toolchain"].version, "^2.2");
    assert_eq!(requirements["node"].implementation, "node");
    assert_eq!(requirements["node"].version, ">=20 <23");
}

#[test]
fn an_explicit_node_requirement_overrides_engines_node() {
    let manifest = serde_json::json!({
        "engines": { "node": ">=18" },
        "pnpm": { "vcs": { "requirements": {
            "node": { "implementation": "node", "version": ">=22" }
        } } }
    });

    let requirements = read_component_requirements(&manifest)
        .expect("read requirements")
        .expect("requirements exist");

    assert_eq!(requirements["node"].version, ">=22");
}

#[test]
fn normalizes_npm_names_for_bit_component_ids() {
    assert_eq!(sanitize_component_name("@Acme/a.package"), "acme/a-package");
}
