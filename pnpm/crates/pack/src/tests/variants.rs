//! Packing the archives `publishConfig.variants` declares.

use super::{
    Host, PackError, PackOptions, fixture, tarball_entry_content, tarball_entry_names, touch,
};
use crate::{PackedArchive, pack_archives};
use pnpm_reporter::SilentReporter;
use serde_json::{Value, json};

fn pack(opts: &PackOptions, targets: &[&str]) -> Result<Vec<PackedArchive>, PackError> {
    let targets: Vec<String> = targets
        .iter()
        .map(|target| (*target).to_string())
        .collect();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(pack_archives::<SilentReporter, Host>(opts, &targets))
}

fn addon() -> Value {
    json!({
        "name": "example-native-addon",
        "version": "2.3.0",
        "main": "index.js",
        "files": ["index.js"],
        "publishConfig": {
            "variants": [
                { "targets": ["any"], "files": ["lib/**"] },
                { "targets": ["linux-x64"], "files": ["addon.linux-x64.node"] },
                {
                    "targets": ["linux-x64-musl"],
                    "files": ["addon.linux-x64-musl.node"],
                    "manifest": { "main": "musl.js" },
                },
            ],
        },
    })
}

fn write_addon(dir: &std::path::Path) {
    for file in ["index.js", "lib/fallback.js", "addon.linux-x64.node", "addon.linux-x64-musl.node"]
    {
        touch(dir, file, file);
    }
}

#[test]
fn packs_one_archive_per_entry_with_the_files_it_claims() {
    let (dir, opts) = fixture(&addon());
    write_addon(dir.path());
    let packed = pack(&opts, &[]).unwrap();
    let names: Vec<&str> = packed
        .iter()
        .map(|archive| archive.result.tarball_path.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "example-native-addon-2.3.0.tgz",
            "example-native-addon-2.3.0~linux-x64.tgz",
            "example-native-addon-2.3.0~linux-x64-musl.tgz",
        ],
    );
    let entries = |name: &str| {
        let mut entries = tarball_entry_names(&dir.path().join(name));
        entries.sort();
        entries
    };
    assert_eq!(
        entries(names[0]),
        ["package/index.js", "package/lib/fallback.js", "package/package.json"],
    );
    assert_eq!(
        entries(names[1]),
        ["package/addon.linux-x64.node", "package/index.js", "package/package.json"],
    );
    let musl: Value = serde_json::from_str(
        &tarball_entry_content(&dir.path().join(names[2]), "package/package.json").unwrap(),
    )
    .unwrap();
    assert_eq!(musl["main"], "musl.js");
    assert_eq!(musl["variant"], json!({ "platforms": ["linux-x64-musl"] }));
    assert!(musl.get("publishConfig").is_none(), "{musl}");
    let primary: Value = serde_json::from_str(
        &tarball_entry_content(&dir.path().join(names[0]), "package/package.json").unwrap(),
    )
    .unwrap();
    assert!(primary.get("variant").is_none(), "{primary}");
}

#[test]
fn packs_only_the_named_archive_without_the_other_entries_files() {
    let (dir, opts) = fixture(&addon());
    touch(dir.path(), "index.js", "index");
    touch(dir.path(), "addon.linux-x64-musl.node", "musl");
    let packed = pack(&opts, &["linux-x64-musl"]).unwrap();
    assert_eq!(packed.len(), 1);
    assert_eq!(packed[0].result.tarball_path, "example-native-addon-2.3.0~linux-x64-musl.tgz");
    assert_eq!(
        packed[0].entry
            .as_ref()
            .unwrap()
            .tag
            .as_deref(),
        Some("linux-x64-musl"),
    );
}

#[test]
fn a_pattern_that_matches_nothing_fails_its_archive() {
    let (dir, opts) = fixture(&addon());
    touch(dir.path(), "index.js", "index");
    touch(dir.path(), "lib/fallback.js", "fallback");
    match pack(&opts, &["linux-x64"]) {
        Err(PackError::VariantPatternMatchesNothing { targets, pattern }) => {
            assert_eq!((targets.as_str(), pattern.as_str()), ("linux-x64", "addon.linux-x64.node"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_variant_that_inherits_a_build_is_refused() {
    let mut manifest = addon();
    manifest["scripts"] = json!({ "install": "node-gyp rebuild" });
    let (dir, opts) = fixture(&manifest);
    write_addon(dir.path());
    assert!(matches!(pack(&opts, &["linux-x64"]), Err(PackError::VariantArchive(_))));
}

#[test]
fn a_package_without_variants_packs_its_archive_and_names_none() {
    let (dir, opts) = fixture(&json!({ "name": "plain", "version": "1.0.0" }));
    touch(dir.path(), "index.js", "index");
    let packed = pack(&opts, &[]).unwrap();
    assert!(packed[0].entry.is_none());
    assert!(matches!(pack(&opts, &["linux-x64"]), Err(PackError::NoVariants { .. })));
}
