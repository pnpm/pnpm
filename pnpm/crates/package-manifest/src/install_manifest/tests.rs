use super::{ArchiveFacts, INSTALL_MANIFEST_PROFILE, InstallManifest};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};

/// The shared fixtures that define [`INSTALL_MANIFEST_PROFILE`]: each one holds
/// an archive's `package.json` and file list, the version metadata a publisher
/// submitted beside it, and the expected derivation and comparison.
fn fixtures_dir() -> PathBuf {
    let profile_dir = INSTALL_MANIFEST_PROFILE.replace('/', "-");
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(profile_dir)
}

fn archive_files(fixture: &Value) -> BTreeSet<String> {
    fixture["archive"]["files"]
        .as_array()
        .expect("fixture lists the archive's files")
        .iter()
        .map(|file| file.as_str().unwrap().to_string())
        .collect()
}

fn check_fixture(name: &str, fixture: &Value) {
    let files = archive_files(fixture);
    let facts = ArchiveFacts {
        binding_gyp: files.contains("binding.gyp"),
        has_shrinkwrap: files.contains("npm-shrinkwrap.json"),
        files: &files,
    };
    let archive = InstallManifest::derive(&fixture["archive"]["packageJson"], facts);
    let submitted = InstallManifest::derive(&fixture["submitted"], facts);
    let expected = &fixture["expected"];
    assert_eq!(serde_json::to_value(&archive).unwrap(), expected["manifest"], "{name}: derived");
    let comparison = submitted.compare(&archive);
    let mismatched: Vec<Value> = comparison.mismatched
        .iter()
        .map(|field| json!(field))
        .collect();
    let tolerated: Vec<Value> = comparison.tolerated
        .iter()
        .map(|field| json!(field))
        .collect();
    assert_eq!(json!(mismatched), expected["mismatched"], "{name}: mismatched");
    assert_eq!(json!(tolerated), expected["tolerated"], "{name}: tolerated");
    assert_eq!(comparison.is_accepted(), comparison.mismatched.is_empty());
}

#[test]
fn every_fixture_derives_and_compares_as_expected() {
    let mut checked = 0;
    for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let fixture: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let name = path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        check_fixture(&name, &fixture);
        checked += 1;
    }
    assert!(checked >= 20, "expected the profile fixtures, found {checked}");
}

#[test]
fn a_manifest_compares_equal_to_itself() {
    let files = BTreeSet::from(["package.json".to_string(), "binding.gyp".to_string()]);
    let facts = ArchiveFacts { binding_gyp: true, has_shrinkwrap: false, files: &files };
    let manifest = json!({
        "name": "widgets",
        "version": "1.0.0",
        "dependencies": { "helper": "^1.0.0" },
        "scripts": { "postinstall": "node setup.js" },
    });
    let derived = InstallManifest::derive(&manifest, facts);
    assert!(derived.compare(&derived).is_accepted());
    assert!(derived.has_install_script());
}

#[test]
fn write_to_version_replaces_install_critical_fields_only() {
    let files = BTreeSet::from(["package.json".to_string(), "cli.js".to_string()]);
    let facts = ArchiveFacts { binding_gyp: false, has_shrinkwrap: true, files: &files };
    let archive = json!({
        "name": "widgets",
        "version": "1.0.0",
        "bin": "./cli.js",
        "optionalDependencies": { "helper": "1.0.0" },
    });
    let derived = InstallManifest::derive(&archive, facts);
    let mut version = json!({
        "name": "widgets",
        "version": "1.0.0",
        "description": "kept",
        "gypfile": true,
        "bin": { "widgets": "cli.js" },
        "dependencies": { "helper": "1.0.0" },
        "optionalDependencies": { "helper": "1.0.0" },
        "bundledDependencies": [],
        "scripts": { "install": "node-gyp rebuild", "test": "node test.js" },
        "dist": { "integrity": "sha512-AAAA" },
    });
    derived.write_to_version(version.as_object_mut().unwrap());
    assert_eq!(
        version,
        json!({
            "name": "widgets",
            "version": "1.0.0",
            "description": "kept",
            "gypfile": true,
            "bin": { "widgets": "cli.js" },
            "optionalDependencies": { "helper": "1.0.0" },
            "scripts": { "test": "node test.js" },
            "dist": { "integrity": "sha512-AAAA" },
            "hasInstallScript": false,
            "_hasShrinkwrap": true,
        }),
    );
}

#[test]
fn the_profile_name_is_stable() {
    assert_eq!(INSTALL_MANIFEST_PROFILE, "npm-install-manifest/1");
}
