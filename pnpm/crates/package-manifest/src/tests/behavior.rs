use super::{
    BINDING_GYP, DependencyGroup, InitOptions, NamedTempFile, PackageManifest, Write, assert_eq,
    files_build_triggers, json, manifest_opts_out_of_gyp_build, manifest_requires_build,
    pkg_requires_build, read_to_string, tempdir,
};
use crate::BuildTriggers;

#[cfg(unix)]
use super::{PackageManifestError, safe_read_package_json_from_dir};

#[cfg(unix)]
#[test]
fn save_leaves_the_original_intact_when_the_write_cannot_complete() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let original = r#"{"name":"intact","version":"1.0.0"}"#;
    std::fs::write(&path, original).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();

    // Make the directory read-only so a sibling temp file cannot be created.
    // An atomic temp-file-then-rename write fails up front and leaves the
    // original untouched; a non-atomic in-place write would instead truncate
    // the existing package.json before failing.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = manifest.save();
    // Restore permissions before the assertions so tempdir cleanup succeeds.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn init_should_throw_if_exists() {
    let tmp = NamedTempFile::new().unwrap();
    write!(tmp.as_file(), "hello world").unwrap();
    PackageManifest::init(tmp.path(), InitOptions::default())
        .expect_err("package.json already exist");
}

/// The scaffold `pnpm add` writes when there is no manifest yet carries no
/// pin — only `pnpm init` pins the project to a pnpm version.
#[test]
fn create_if_needed_does_not_pin_a_package_manager() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    PackageManifest::create_if_needed(tmp.clone()).unwrap();
    let contents = read_to_string(tmp).unwrap();
    assert!(!contents.contains("packageManager"), "{contents}");
    assert!(!contents.contains("devEngines"), "{contents}");
}

#[test]
fn should_throw_on_missing_command() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(tmp).unwrap();
    manifest.script("dev", false).expect_err("dev command should not exist");
}

#[test]
fn should_execute_a_command() {
    let data = r#"
    {
        "scripts": {
            "test": "echo"
        }
    }
    "#;
    let tmp = NamedTempFile::new().unwrap();
    write!(tmp.as_file(), "{data}").unwrap();
    let manifest = PackageManifest::create_if_needed(tmp.path().to_path_buf()).unwrap();
    assert_eq!(manifest.script("test", false).unwrap(), Some("echo"));
    manifest.script("invalid", false).expect_err("invalid command should not exist");
    assert_eq!(manifest.script("invalid", true).unwrap(), None);
}

#[cfg(unix)]
#[test]
fn safe_read_surfaces_non_not_found_io_errors() {
    let dir = tempdir().unwrap();
    // Plant `package.json` as a directory rather than a file.
    // `fs::read_to_string` returns `IsADirectory`, never `NotFound`.
    std::fs::create_dir(dir.path().join("package.json")).unwrap();

    let err = safe_read_package_json_from_dir(dir.path())
        .expect_err("read_to_string on a directory should fail");
    assert!(matches!(err, PackageManifestError::Io(_)), "expected Io error, got {err:?}");
}

#[test]
fn from_value_coerces_non_object_input_to_an_empty_object() {
    // A non-object manifest supplied across the FFI boundary must not later
    // panic when a dependency is inserted; `from_value` normalizes it to `{}`.
    for value in [json!([1, 2, 3]), json!("oops"), json!(42), json!(true), json!(null)] {
        let mut manifest = PackageManifest::from_value("/x/package.json".into(), value.clone());
        assert!(manifest.value().is_object(), "input {value} should normalize to an object");
        manifest
            .add_dependency("is-odd", "3.0.1", DependencyGroup::Prod)
            .expect("adding a dependency to a normalized manifest must not fail");
    }
}

/// A save re-serializes with the source file's own indentation unit: tabs
/// stay tabs, wider space units stay wide, and a single-line document
/// stays compact.
#[test]
fn save_preserves_the_source_indentation() {
    let dir = tempdir().unwrap();

    let cases = [
        ("tabs.json", "{\n\t\"name\": \"foo\"\n}\n", "\t\"name\""),
        ("wide.json", "{\n    \"name\": \"foo\"\n}\n", r#"    "name""#),
    ];
    for (file_name, source, expected_fragment) in cases {
        let path = dir.path().join(file_name);
        std::fs::write(&path, source).unwrap();
        let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
        manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
        manifest.save().unwrap();
        let saved = read_to_string(&path).unwrap();
        eprintln!("{file_name} SAVED:\n{saved}");
        assert!(saved.contains(expected_fragment));
        assert!(saved.contains("fastify"));
    }

    let single_line = dir.path().join("single-line.json");
    std::fs::write(&single_line, r#"{"name":"foo"}"#).unwrap();
    let mut manifest = PackageManifest::from_path(single_line.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert_eq!(
        read_to_string(&single_line).unwrap(),
        r#"{"name":"foo","dependencies":{"fastify":"1.0.0"}}"#,
    );
}

/// A save keeps the blank lines the source file puts between object
/// members, at the top level and in nested objects.
#[test]
fn save_preserves_blank_lines_between_members() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(
        &path,
        "{\n  \"name\": \"foo\",\n  \"scripts\": {\n    \"test\": \"a\",\n\n    \"foo\": \"bar\"\n  },\n\n  \"license\": \"ISC\"\n}\n",
    )
    .unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("axios", "^1.1.3", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    assert_eq!(
        read_to_string(&path).unwrap(),
        "{\n  \"name\": \"foo\",\n  \"scripts\": {\n    \"test\": \"a\",\n\n    \"foo\": \"bar\"\n  },\n\n  \"license\": \"ISC\",\n  \"dependencies\": {\n    \"axios\": \"^1.1.3\"\n  }\n}\n",
    );
}

/// A member removed by one save comes back without its old blank line
/// when the same manifest adds it again.
#[test]
fn save_forgets_the_blank_line_of_a_removed_member() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(
        &path,
        "{\n  \"name\": \"foo\",\n  \"dependencies\": {\n    \"a\": \"1.0.0\",\n\n    \"b\": \"1.0.0\"\n  }\n}\n",
    )
    .unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.remove_dependencies(&["b".to_string()], None);
    manifest.save().unwrap();
    manifest.add_dependency("b", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    assert_eq!(
        read_to_string(&path).unwrap(),
        "{\n  \"name\": \"foo\",\n  \"dependencies\": {\n    \"a\": \"1.0.0\",\n    \"b\": \"1.0.0\"\n  }\n}\n",
    );
}

/// The preserved indentation unit is capped at 10 characters on write,
/// like `JSON.stringify`'s `space` argument.
#[test]
fn save_caps_the_indentation_unit_at_ten_characters() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let twelve_spaces = " ".repeat(12);
    std::fs::write(&path, format!("{{\n{twelve_spaces}\"name\": \"foo\"\n}}\n")).unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let saved = read_to_string(&path).unwrap();
    eprintln!("SAVED:\n{saved}");
    assert!(saved.contains(&format!("\n{}\"name\"", " ".repeat(10))));
    assert!(!saved.contains(&format!("\n{twelve_spaces}\"name\"")));
}

/// pnpm v11's `pkgRequiresBuild` reads the script's value, not the key, so
/// a manifest that carries an empty one is build-free in both stacks.
#[test]
fn a_script_without_a_value_is_not_build_work() {
    assert!(manifest_requires_build(&json!({ "scripts": { "postinstall": "node x.js" } })));
    assert!(!manifest_requires_build(&json!({ "scripts": { "postinstall": "" } })));
    assert!(!manifest_requires_build(
        &json!({ "scripts": { "install": serde_json::Value::Null } })
    ));
    assert!(!manifest_requires_build(&json!({ "scripts": { "preinstall": false } })));
    assert!(!manifest_requires_build(&json!({ "scripts": { "test": "node x.js" } })));
}

/// Only `gypfile: false` opts out. npm writes `gypfile: true` at publish time
/// for every package it synthesizes the install script for, so that value says
/// nothing pnpm did not already know from the `binding.gyp` itself.
#[test]
fn only_a_false_gypfile_opts_out_of_the_gyp_build() {
    assert!(manifest_opts_out_of_gyp_build(&json!({ "gypfile": false })));
    assert!(!manifest_opts_out_of_gyp_build(&json!({ "gypfile": true })));
    assert!(!manifest_opts_out_of_gyp_build(&json!({ "gypfile": "false" })));
    assert!(!manifest_opts_out_of_gyp_build(&json!({})));
}

/// `gypfile` speaks for the synthesized `node-gyp rebuild` alone, so it silences
/// a `binding.gyp` and nothing else.
#[test]
fn gypfile_false_silences_only_the_binding_gyp_trigger() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    std::fs::write(pkg_root.join(BINDING_GYP), "{'targets':[]}").expect("write binding.gyp");

    std::fs::write(pkg_root.join("package.json"), json!({ "gypfile": false }).to_string())
        .expect("write manifest");
    assert!(!pkg_requires_build(pkg_root));

    std::fs::write(pkg_root.join("package.json"), json!({}).to_string()).expect("write manifest");
    assert!(pkg_requires_build(pkg_root));

    std::fs::write(
        pkg_root.join("package.json"),
        json!({ "gypfile": false, "scripts": { "postinstall": "node x.js" } }).to_string(),
    )
    .expect("write manifest");
    assert!(pkg_requires_build(pkg_root));

    std::fs::create_dir(pkg_root.join(".hooks")).expect("create .hooks");
    std::fs::write(pkg_root.join("package.json"), json!({ "gypfile": false }).to_string())
        .expect("write manifest");
    assert!(pkg_requires_build(pkg_root));
}

/// A package whose files carry a `binding.gyp` reports it apart from a
/// `.hooks/` entry, so a caller that reads the manifest after the files can
/// still apply the opt-out.
#[test]
fn file_triggers_report_binding_gyp_and_hooks_separately() {
    let gyp = files_build_triggers([BINDING_GYP, "lib/index.js"]);
    assert!(gyp.binding_gyp);
    assert!(!gyp.hooks);
    assert!(gyp.requires_build());
    assert!(!BuildTriggers { gyp_build_opted_out: true, ..gyp }.requires_build());

    let hooks = files_build_triggers([".hooks/postinstall"]);
    assert!(hooks.hooks);
    assert!(!hooks.binding_gyp);
    assert!(BuildTriggers { gyp_build_opted_out: true, ..hooks }.requires_build());

    assert!(!files_build_triggers(["lib/binding.gyp", ".hooksfile"]).requires_build());
}
