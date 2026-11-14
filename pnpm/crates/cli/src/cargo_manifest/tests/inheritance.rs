use super::super::{CargoDependencyKind, add_dependencies};
use std::fs;

#[test]
fn add_dependencies_preserves_inherited_workspace_settings_when_detaching() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let member = root.join("member");
    fs::create_dir_all(&member).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        concat!(
            "[workspace]\n",
            "members = [\"member\"]\n\n",
            "[workspace.dependencies]\n",
            "my_serde = { package = \"serde\", version = \"1.0\", default-features = false, features = [\"derive\"] }\n",
            "tokio = { version = \"1.0\", features = [\"rt\"] }\n",
        ),
    )
    .unwrap();
    let member_manifest = member.join("Cargo.toml");
    fs::write(
        &member_manifest,
        concat!(
            "[package]\n",
            "name = \"member\"\n",
            "version = \"0.1.0\"\n\n",
            "[dependencies]\n",
            "my_serde = { workspace = true, features = [\"rc\"], optional = true }\n\n",
            "[dependencies.tokio]\n",
            "workspace = true\n",
        ),
    )
    .unwrap();

    add_dependencies(
        &member_manifest,
        &[
            ("my_serde".to_string(), "1.0.228".to_string()),
            ("tokio".to_string(), "1.43.0".to_string()),
        ],
        CargoDependencyKind::Normal,
    )
    .unwrap();

    let updated = fs::read_to_string(&member_manifest).unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table = toml::from_str(&updated).unwrap();
    let serde_dep = parsed["dependencies"]["my_serde"].as_table().unwrap();
    assert_eq!(serde_dep.get("workspace"), None);
    assert_eq!(serde_dep["version"].as_str(), Some("1.0.228"));
    assert_eq!(serde_dep["package"].as_str(), Some("serde"));
    assert_eq!(serde_dep["default-features"].as_bool(), Some(false));
    assert_eq!(serde_dep["optional"].as_bool(), Some(true));
    assert_eq!(
        serde_dep["features"].as_array().map(Vec::as_slice),
        Some(["derive".into(), "rc".into()].as_slice()),
    );

    let tokio_dep = parsed["dependencies"]["tokio"].as_table().unwrap();
    assert_eq!(tokio_dep.get("workspace"), None);
    assert_eq!(tokio_dep["version"].as_str(), Some("1.43.0"));
    assert_eq!(tokio_dep["features"].as_array().map(Vec::as_slice), Some(["rt".into()].as_slice()));
}

#[test]
fn add_dependencies_preserves_inherited_registry_git_and_path_sources() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let app = root.join("crates/app");
    fs::create_dir_all(&app).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        concat!(
            "[workspace]\n",
            "members = [\"crates/*\"]\n\n",
            "[workspace.dependencies]\n",
            "priv_reg = { version = \"0.1.0\", registry = \"corp\" }\n",
            "direct_idx = { version = \"0.1.0\", registry-index = \"https://registry.example.test/index/\" }\n",
            "git_dep = { git = \"https://example.test/repo.git\", tag = \"v1.0.0\" }\n",
            "shared = { path = \"crates/shared\" }\n",
        ),
    )
    .unwrap();
    let app_manifest = app.join("Cargo.toml");
    fs::write(
        &app_manifest,
        concat!(
            "[package]\n",
            "name = \"app\"\n",
            "version = \"0.1.0\"\n\n",
            "[dependencies]\n",
            "priv_reg = { workspace = true }\n",
            "direct_idx = { workspace = true }\n",
            "git_dep = { workspace = true }\n",
            "shared = { workspace = true }\n",
        ),
    )
    .unwrap();

    add_dependencies(
        &app_manifest,
        &[
            ("priv_reg".to_string(), "0.2.0".to_string()),
            ("direct_idx".to_string(), "0.2.0".to_string()),
            ("git_dep".to_string(), "1.0.0".to_string()),
            ("shared".to_string(), "0.3.0".to_string()),
        ],
        CargoDependencyKind::Normal,
    )
    .unwrap();

    let updated = fs::read_to_string(&app_manifest).unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table = toml::from_str(&updated).unwrap();
    let deps = parsed["dependencies"].as_table().unwrap();
    assert_eq!(deps["priv_reg"]["registry"].as_str(), Some("corp"));
    assert_eq!(deps["priv_reg"]["version"].as_str(), Some("0.2.0"));
    assert_eq!(
        deps["direct_idx"]["registry-index"].as_str(),
        Some("https://registry.example.test/index/"),
    );
    assert_eq!(deps["git_dep"]["git"].as_str(), Some("https://example.test/repo.git"));
    assert_eq!(deps["git_dep"]["tag"].as_str(), Some("v1.0.0"));
    assert_eq!(deps["git_dep"]["version"].as_str(), Some("1.0.0"));
    assert_eq!(deps["shared"]["path"].as_str(), Some("../shared"));
    assert_eq!(deps["shared"]["version"].as_str(), Some("0.3.0"));
}

#[test]
fn add_dependencies_resolves_package_workspace_outside_ancestors() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let external = dir.path().join("external");
    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(&external).unwrap();
    fs::write(
        ws.join("Cargo.toml"),
        concat!(
            "[workspace]\n",
            "members = [\"../external\"]\n\n",
            "[workspace.dependencies]\n",
            "shared = { package = \"real_shared\", path = \"crates/shared\", default-features = false, features = [\"std\"] }\n",
        ),
    )
    .unwrap();
    let ext_manifest = external.join("Cargo.toml");
    fs::write(
        &ext_manifest,
        concat!(
            "[package]\n",
            "name = \"external\"\n",
            "version = \"0.1.0\"\n",
            "workspace = \"../ws\"\n\n",
            "[dependencies]\n",
            "shared = { workspace = true }\n",
        ),
    )
    .unwrap();

    add_dependencies(
        &ext_manifest,
        &[("shared".to_string(), "0.2.0".to_string())],
        CargoDependencyKind::Normal,
    )
    .unwrap();

    let updated = fs::read_to_string(&ext_manifest).unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table = toml::from_str(&updated).unwrap();
    let shared = parsed["dependencies"]["shared"].as_table().unwrap();
    assert_eq!(shared["version"].as_str(), Some("0.2.0"));
    assert_eq!(shared["package"].as_str(), Some("real_shared"));
    assert_eq!(shared["path"].as_str(), Some("../ws/crates/shared"));
    assert_eq!(shared["default-features"].as_bool(), Some(false));
    assert_eq!(shared["features"].as_array().map(Vec::as_slice), Some(["std".into()].as_slice()));
}

#[test]
fn add_dependencies_errors_when_package_workspace_is_invalid_or_excludes_member() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let outsider = dir.path().join("outsider");
    let missing_ws = dir.path().join("missing-ws");
    for path in [&ws, &outsider, &missing_ws] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nfoo = \"1.0\"\n",
    )
    .unwrap();
    let outsider_manifest = outsider.join("Cargo.toml");
    fs::write(
        &outsider_manifest,
        "[package]\nname = \"outsider\"\nversion = \"0.1.0\"\nworkspace = \"../ws\"\n\n[dependencies]\nfoo = { workspace = true }\n",
    )
    .unwrap();
    let err = add_dependencies(
        &outsider_manifest,
        &[("foo".to_string(), "1.1.0".to_string())],
        CargoDependencyKind::Normal,
    )
    .unwrap_err();
    assert!(err.to_string().contains("does not include"), "{err:#}");

    let missing_manifest = missing_ws.join("Cargo.toml");
    fs::write(
        &missing_manifest,
        "[package]\nname = \"missing\"\nversion = \"0.1.0\"\nworkspace = \"../nonexistent\"\n\n[dependencies]\nfoo = { workspace = true }\n",
    )
    .unwrap();
    let missing_err = add_dependencies(
        &missing_manifest,
        &[("foo".to_string(), "1.1.0".to_string())],
        CargoDependencyKind::Normal,
    )
    .unwrap_err();
    assert!(missing_err.to_string().contains("cannot detach crate foo"), "{missing_err:#}");
}

#[test]
fn add_dependencies_strips_ignored_member_sources_and_package_overrides_when_detaching() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    fs::create_dir_all(&app).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        concat!(
            "[workspace]\n",
            "members = [\"app\"]\n\n",
            "[workspace.dependencies]\n",
            "itoa = \"1.0\"\n",
            "aliased = { version = \"1.0\", package = \"real_pkg\", registry = \"corp\" }\n",
            "git_dep = { git = \"https://trusted.example/repo.git\", tag = \"v1.0.0\" }\n",
        ),
    )
    .unwrap();
    let app_manifest = app.join("Cargo.toml");
    fs::write(
        &app_manifest,
        concat!(
            "[package]\n",
            "name = \"app\"\n",
            "version = \"0.1.0\"\n\n",
            "[dependencies]\n",
            "itoa = { workspace = true, git = \"https://untrusted.example/itoa.git\", branch = \"main\", package = \"wrong_pkg\", optional = true }\n",
            "git_dep.workspace = true\n",
            "git_dep.git = \"https://untrusted.example/other.git\"\n",
            "git_dep.rev = \"deadbeef\"\n\n",
            "[dependencies.aliased]\n",
            "workspace = true\n",
            "path = \"../untrusted\"\n",
            "registry = \"wrong\"\n",
            "package = \"wrong_pkg\"\n",
            "features = [\"extra\"]\n",
        ),
    )
    .unwrap();

    add_dependencies(
        &app_manifest,
        &[
            ("itoa".to_string(), "1.0.18".to_string()),
            ("aliased".to_string(), "1.1.0".to_string()),
            ("git_dep".to_string(), "1.0.1".to_string()),
        ],
        CargoDependencyKind::Normal,
    )
    .unwrap();

    let updated = fs::read_to_string(&app_manifest).unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table = toml::from_str(&updated).unwrap();
    let deps = parsed["dependencies"].as_table().unwrap();

    let itoa = deps["itoa"].as_table().unwrap();
    assert_eq!(itoa.get("version").and_then(toml::Value::as_str), Some("1.0.18"));
    assert_eq!(itoa.get("optional").and_then(toml::Value::as_bool), Some(true));
    assert!(!itoa.contains_key("git"));
    assert!(!itoa.contains_key("branch"));
    assert!(!itoa.contains_key("package"));

    let aliased = deps["aliased"].as_table().unwrap();
    assert_eq!(aliased.get("version").and_then(toml::Value::as_str), Some("1.1.0"));
    assert_eq!(aliased.get("package").and_then(toml::Value::as_str), Some("real_pkg"));
    assert_eq!(aliased.get("registry").and_then(toml::Value::as_str), Some("corp"));
    assert!(!aliased.contains_key("path"));

    let git_dep = deps["git_dep"].as_table().unwrap();
    assert_eq!(git_dep.get("version").and_then(toml::Value::as_str), Some("1.0.1"));
    assert_eq!(
        git_dep.get("git").and_then(toml::Value::as_str),
        Some("https://trusted.example/repo.git"),
    );
    assert_eq!(git_dep.get("tag").and_then(toml::Value::as_str), Some("v1.0.0"));
    assert!(!git_dep.contains_key("rev"));
}
