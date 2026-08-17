//! Round-trip tests for reading and writing the `.modules.yaml`
//! manifest.
//!
//! Further `.modules.yaml` behavior-branch tests live in sibling files
//! (`real_fs.rs`, `fakes.rs`).

use indexmap::IndexMap;
use pipe_trait::Pipe;
use pnpm_modules_yaml::{HoistKind, Host, Modules, read_modules_manifest, write_modules_manifest};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{fs, path::Path};

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
        IndexMap::from([
            (
                "/accepts/1.3.7".to_string(),
                IndexMap::from([("accepts".to_string(), HoistKind::Public)]),
            ),
            (
                "/array-flatten/1.1.1".to_string(),
                IndexMap::from([("array-flatten".to_string(), HoistKind::Public)]),
            ),
            (
                "/body-parser/1.19.0".to_string(),
                IndexMap::from([("body-parser".to_string(), HoistKind::Public)]),
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
        IndexMap::from([
            (
                "/accepts/1.3.7".to_string(),
                IndexMap::from([("accepts".to_string(), HoistKind::Private)]),
            ),
            (
                "/array-flatten/1.1.1".to_string(),
                IndexMap::from([("array-flatten".to_string(), HoistKind::Private)]),
            ),
            (
                "/body-parser/1.19.0".to_string(),
                IndexMap::from([("body-parser".to_string(), HoistKind::Private)]),
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
fn write_modules_manifest_preserves_hoisted_dependency_order() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path();
    let manifest = Modules {
        hoisted_dependencies: IndexMap::from([
            (
                "z@1.0.0".to_string(),
                IndexMap::from([
                    ("z-alias".to_string(), HoistKind::Private),
                    ("a-alias".to_string(), HoistKind::Private),
                ]),
            ),
            ("a@1.0.0".to_string(), IndexMap::from([("a".to_string(), HoistKind::Private)])),
        ]),
        virtual_store_dir: modules_dir.join(".pnpm").display().to_string(),
        ..Default::default()
    };

    write_modules_manifest::<Host>(modules_dir, manifest).expect("write manifest");
    let written = read_modules_manifest::<Host>(modules_dir)
        .expect("read manifest")
        .expect("manifest exists");

    assert_eq!(
        written.hoisted_dependencies.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["z@1.0.0", "a@1.0.0"],
    );
    assert_eq!(
        written.hoisted_dependencies["z@1.0.0"].keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["z-alias", "a-alias"],
    );
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
