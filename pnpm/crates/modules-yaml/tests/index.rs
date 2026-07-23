//! Round-trip tests for reading and writing the `.modules.yaml`
//! manifest.
//!
//! Further `.modules.yaml` behavior-branch tests live in sibling files
//! (`real_fs.rs`, `fakes.rs`).

use pacquet_modules_yaml::{
    HoistKind, Host, Modules, read_modules_manifest, write_modules_manifest,
};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn manifest_from_json(value: Value) -> Modules {
    serde_json::from_value(value).expect("deserialize Modules fixture")
}

#[test]
fn write_modules_manifest_and_read_modules_manifest() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path();
    let modules_yaml = manifest_from_json(json!({
        "hoistedDependencies": {},
        "included": {
            "dependencies": true,
            "devDependencies": true,
            "optionalDependencies": true,
        },
        "ignoredBuilds": [],
        "layoutVersion": 5,
        "packageManager": "pnpm@2",
        "pendingBuilds": [],
        "publicHoistPattern": [],
        "prunedAt": "Thu, 01 Jan 1970 00:00:00 GMT",
        "registries": {
            "default": "https://registry.npmjs.org/",
        },
        "shamefullyHoist": false,
        "skipped": [],
        "storeDir": "/.pnpm-store",
        "virtualStoreDir": modules_dir.join(".pnpm"),
        "virtualStoreDirMaxLength": 120,
    }));

    write_modules_manifest::<Host>(modules_dir, modules_yaml.clone()).expect("write manifest");
    let actual = read_modules_manifest::<Host>(modules_dir).expect("read manifest");
    assert_eq!(actual, Some(modules_yaml));

    let raw: Value = modules_dir
        .join(".modules.yaml")
        .pipe(fs::read_to_string)
        .expect("read raw .modules.yaml")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse raw .modules.yaml");
    let virtual_store_dir = raw
        .get("virtualStoreDir")
        .expect("virtualStoreDir is present")
        .as_str()
        .expect("virtualStoreDir is a string")
        .pipe(Path::new);
    assert_eq!(virtual_store_dir.is_absolute(), cfg!(windows));
}

#[test]
fn read_legacy_shamefully_hoist_true_manifest() {
    let manifest = env!("CARGO_MANIFEST_DIR")
        .pipe(Path::new)
        .join("tests/fixtures/old-shamefully-hoist")
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read manifest")
        .expect("modules manifest exists");

    assert_eq!(manifest.public_hoist_pattern.as_deref(), Some(&["*".to_string()][..]));
    assert_eq!(
        manifest.hoisted_dependencies,
        BTreeMap::from([
            (
                "/accepts/1.3.7".to_string(),
                BTreeMap::from([("accepts".to_string(), HoistKind::Public)]),
            ),
            (
                "/array-flatten/1.1.1".to_string(),
                BTreeMap::from([("array-flatten".to_string(), HoistKind::Public)]),
            ),
            (
                "/body-parser/1.19.0".to_string(),
                BTreeMap::from([("body-parser".to_string(), HoistKind::Public)]),
            ),
        ]),
    );
}

#[test]
fn read_legacy_shamefully_hoist_false_manifest() {
    let manifest = env!("CARGO_MANIFEST_DIR")
        .pipe(Path::new)
        .join("tests/fixtures/old-no-shamefully-hoist")
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read manifest")
        .expect("modules manifest exists");

    assert_eq!(manifest.public_hoist_pattern.as_deref(), Some(&[][..]));
    assert_eq!(
        manifest.hoisted_dependencies,
        BTreeMap::from([
            (
                "/accepts/1.3.7".to_string(),
                BTreeMap::from([("accepts".to_string(), HoistKind::Private)]),
            ),
            (
                "/array-flatten/1.1.1".to_string(),
                BTreeMap::from([("array-flatten".to_string(), HoistKind::Private)]),
            ),
            (
                "/body-parser/1.19.0".to_string(),
                BTreeMap::from([("body-parser".to_string(), HoistKind::Private)]),
            ),
        ]),
    );
}

#[test]
fn write_modules_manifest_creates_node_modules_directory() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path().join("node_modules");
    let modules_yaml = manifest_from_json(json!({
        "hoistedDependencies": {},
        "included": {
            "dependencies": true,
            "devDependencies": true,
            "optionalDependencies": true,
        },
        "ignoredBuilds": [],
        "layoutVersion": 5,
        "packageManager": "pnpm@2",
        "pendingBuilds": [],
        "publicHoistPattern": [],
        "prunedAt": "Thu, 01 Jan 1970 00:00:00 GMT",
        "registries": {
            "default": "https://registry.npmjs.org/",
        },
        "shamefullyHoist": false,
        "skipped": [],
        "storeDir": "/.pnpm-store",
        "virtualStoreDir": modules_dir.join(".pnpm"),
        "virtualStoreDirMaxLength": 120,
    }));

    write_modules_manifest::<Host>(&modules_dir, modules_yaml.clone()).expect("write manifest");
    let actual = read_modules_manifest::<Host>(&modules_dir).expect("read manifest");
    assert_eq!(actual, Some(modules_yaml));
}

#[test]
fn read_empty_modules_manifest_returns_none() {
    let modules_yaml = env!("CARGO_MANIFEST_DIR")
        .pipe(Path::new)
        .join("tests/fixtures/empty-modules-yaml")
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read manifest");
    assert_eq!(modules_yaml, None);
}
