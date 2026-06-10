//! Pacquet-side tests that exercise behavior branches by writing
//! `.modules.yaml` files to a real `tempfile::tempdir()` and reading them
//! back. These cover branches that pnpm only exercises transitively
//! through install-level integration tests in `pnpm/test/`
//! (e.g. custom `virtualStoreDir` at
//! <https://github.com/pnpm/pnpm/blob/1819226b51/pnpm/test/monorepo/index.ts#L1467-L1545>);
//! the install integration tests are gated on the install pipeline being
//! ported, so these direct unit tests guard the behavior in the meantime.

use indexmap::IndexSet;
use pacquet_modules_yaml::{DepPath, Host, Modules, read_modules_manifest, write_modules_manifest};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn manifest_from_json(value: Value) -> Modules {
    serde_json::from_value(value).expect("deserialize Modules fixture")
}

/// Reading a manifest whose `virtualStoreDir` is already absolute must
/// preserve it verbatim, matching upstream
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L66-L70>.
#[test]
fn read_preserves_absolute_virtual_store_dir() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path().join("node_modules");
    fs::create_dir_all(&modules_dir).expect("create modules dir");
    let custom_store = temp_dir.path().join("custom-store");
    let raw = json!({ "virtualStoreDir": &custom_store, "layoutVersion": 5 }).to_string();
    fs::write(modules_dir.join(".modules.yaml"), raw).expect("write fixture");

    let manifest = modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read manifest")
        .expect("manifest exists");
    assert_eq!(Path::new(&manifest.virtual_store_dir), custom_store);
}

/// A non-descendant `virtualStoreDir` (the default macOS / Linux
/// setup, where the global store sits outside the project) must
/// survive a write→read round-trip with its absolute form intact.
///
/// This is what [`crate::Install`]'s no-op short-circuit relies on:
/// the recovered absolute path is compared byte-for-byte against
/// `Config::effective_virtual_store_dir`, and an unnormalized join
/// (`<modules_dir>/../../...`) never matches the normalized config
/// side — so the short-circuit silently misses every install whose
/// store lives outside the project.
#[cfg(not(windows))]
#[test]
fn round_trip_recovers_normalized_absolute_for_non_descendant_store() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path().join("project").join("node_modules");
    let absolute_store = temp_dir.path().join(".pnpm-store");
    let manifest = manifest_from_json(json!({
        "layoutVersion": 5,
        "virtualStoreDir": &absolute_store,
    }));

    write_modules_manifest::<Host>(&modules_dir, manifest).expect("write manifest");
    let actual = read_modules_manifest::<Host>(&modules_dir)
        .expect("read manifest")
        .expect("manifest exists");
    assert_eq!(Path::new(&actual.virtual_store_dir), absolute_store);
}

/// On non-Windows, `write_modules_manifest` rewrites a non-descendant
/// `virtualStoreDir` (sibling, parent, etc.) to a relative path with
/// `..` segments — matching upstream's `path.relative()` output at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L132-L135>,
/// not just the descendant case that `Path::strip_prefix` covers.
#[cfg(not(windows))]
#[test]
fn write_relativizes_non_descendant_virtual_store_dir() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path().join("project").join("node_modules");
    let sibling_store = temp_dir.path().join(".pnpm-store");
    let manifest = manifest_from_json(json!({
        "layoutVersion": 5,
        "virtualStoreDir": &sibling_store,
    }));

    write_modules_manifest::<Host>(&modules_dir, manifest).expect("write manifest");
    let raw: Value = modules_dir
        .join(".modules.yaml")
        .pipe(fs::read_to_string)
        .expect("read raw .modules.yaml")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse raw .modules.yaml");
    assert_eq!(raw["virtualStoreDir"], json!("../../.pnpm-store"));
}

/// `writeModules` sorts `skipped` in place before serializing, matching
/// upstream
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L117>.
#[test]
fn write_sorts_skipped_array() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path();
    let manifest = manifest_from_json(json!({
        "layoutVersion": 5,
        "skipped": ["zeta", "alpha", "mu"],
    }));

    write_modules_manifest::<Host>(modules_dir, manifest).expect("write manifest");
    let raw: Value = modules_dir
        .join(".modules.yaml")
        .pipe(fs::read_to_string)
        .expect("read raw .modules.yaml")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse raw .modules.yaml");
    assert_eq!(raw["skipped"], json!(["alpha", "mu", "zeta"]));
}

/// A null `publicHoistPattern` is removed before serializing because the
/// YAML writer fails on undefined fields upstream. The behavior matches
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L123-L125>.
#[test]
fn write_removes_null_public_hoist_pattern() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path();
    let manifest = manifest_from_json(json!({
        "layoutVersion": 5,
        "publicHoistPattern": null,
    }));

    write_modules_manifest::<Host>(modules_dir, manifest).expect("write manifest");
    let raw: Value = modules_dir
        .join(".modules.yaml")
        .pipe(fs::read_to_string)
        .expect("read raw .modules.yaml")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse raw .modules.yaml");
    assert!(
        raw.get("publicHoistPattern").is_none(),
        "publicHoistPattern was kept after write: {raw}",
    );
}

/// `DepPath` is a transparent newtype around `String`: on the wire it is
/// indistinguishable from a plain string, so `hoistedAliases` keys and
/// `ignoredBuilds` elements round-trip through JSON (and YAML) the same
/// way upstream's `as DepPath`-cast values do.
#[test]
fn dep_path_serializes_transparently() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path();
    let manifest = manifest_from_json(json!({
        "layoutVersion": 5,
        "hoistedAliases": {
            "/accepts/1.3.7": ["accepts"],
        },
        "ignoredBuilds": ["/sharp/0.32.0"],
        "publicHoistPattern": [],
    }));
    assert_eq!(
        manifest.hoisted_aliases.as_ref().and_then(|map| map.keys().next()),
        Some(&DepPath::from("/accepts/1.3.7".to_string())),
    );
    let expected_ignored: IndexSet<DepPath> =
        std::iter::once(DepPath::from("/sharp/0.32.0".to_string())).collect();
    assert_eq!(manifest.ignored_builds.as_ref(), Some(&expected_ignored));

    write_modules_manifest::<Host>(modules_dir, manifest).expect("write manifest");
    let raw: Value = modules_dir
        .join(".modules.yaml")
        .pipe(fs::read_to_string)
        .expect("read raw .modules.yaml")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse raw .modules.yaml");
    assert_eq!(
        raw["hoistedAliases"]["/accepts/1.3.7"],
        json!(["accepts"]),
        "DepPath key did not serialize as a plain string",
    );
    assert_eq!(
        raw["ignoredBuilds"],
        json!(["/sharp/0.32.0"]),
        "DepPath element did not serialize as a plain string",
    );
}

/// `hoistedLocations` is the per-depPath list of lockfile-relative
/// directory paths that `linkHoistedModules` and rebuild consult to
/// find where a package lives on disk. Pacquet has no consumer yet
/// (the install pipeline still writes the field as `None`), so this
/// test pins the schema-level round-trip until a real producer
/// appears. Mirrors the optional `Record<string, string[]>` shape at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/installing/modules-yaml/src/index.ts#L43>.
#[test]
fn hoisted_locations_round_trips() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path();
    let manifest = manifest_from_json(json!({
        "layoutVersion": 5,
        "hoistedLocations": {
            "/accepts/1.3.7": ["node_modules/accepts"],
            "/body-parser/1.19.0": [
                "node_modules/body-parser",
                "node_modules/express/node_modules/body-parser",
            ],
        },
    }));
    assert_eq!(
        manifest.hoisted_locations.as_ref().expect("present").get("/accepts/1.3.7"),
        Some(&vec!["node_modules/accepts".to_string()]),
    );

    write_modules_manifest::<Host>(modules_dir, manifest.clone()).expect("write manifest");
    let actual = read_modules_manifest::<Host>(modules_dir)
        .expect("read manifest")
        .expect("manifest exists");
    assert_eq!(actual.hoisted_locations, manifest.hoisted_locations);

    let raw: Value = modules_dir
        .join(".modules.yaml")
        .pipe(fs::read_to_string)
        .expect("read raw .modules.yaml")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse raw .modules.yaml");
    assert_eq!(
        raw["hoistedLocations"]["/body-parser/1.19.0"],
        json!(["node_modules/body-parser", "node_modules/express/node_modules/body-parser"]),
    );
}

/// A manifest with no `hoistedLocations` (the only state pacquet
/// writes today) must omit the field on disk rather than emit
/// `hoistedLocations: null`. Upstream's `Record<string, string[]> |
/// undefined` shape relies on `JSON.stringify` dropping `undefined`
/// values; pacquet relies on `skip_serializing_if = "Option::is_none"`.
#[test]
fn absent_hoisted_locations_is_omitted_on_write() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let modules_dir = temp_dir.path();
    let manifest = manifest_from_json(json!({ "layoutVersion": 5 }));
    assert!(manifest.hoisted_locations.is_none(), "fixture seed");

    write_modules_manifest::<Host>(modules_dir, manifest).expect("write manifest");
    let raw: Value = modules_dir
        .join(".modules.yaml")
        .pipe(fs::read_to_string)
        .expect("read raw .modules.yaml")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse raw .modules.yaml");
    assert!(raw.get("hoistedLocations").is_none(), "hoistedLocations was emitted when None: {raw}");
}
