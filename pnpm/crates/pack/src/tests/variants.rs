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
        .map(|packed| packed.archives)
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
    for file in [
        "index.js",
        "musl.js",
        "lib/fallback.js",
        "addon.linux-x64.node",
        "addon.linux-x64-musl.node",
    ] {
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
    assert_eq!(
        entries(names[2]),
        [
            "package/addon.linux-x64-musl.node",
            "package/index.js",
            "package/musl.js",
            "package/package.json",
        ],
        "the musl archive holds the main its manifest names",
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
    touch(dir.path(), "musl.js", "musl");
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

/// The RFC's CLI: nothing is shared, the `any` entry claims the JavaScript
/// shim the base `bin` names, and each native archive holds only the
/// executable its own `bin` names.
#[test]
fn an_archive_holds_its_own_entry_points_not_the_base_manifests() {
    let (dir, opts) = fixture(&json!({
        "name": "example-cli",
        "version": "4.0.0",
        "bin": { "example-cli": "bin/example-cli.js" },
        "files": [],
        "publishConfig": {
            "variants": [
                { "targets": ["any"], "files": ["bin/**", "lib/**"] },
                {
                    "targets": ["linux-x64"],
                    "files": ["native/linux-x64/example-cli"],
                    "manifest": { "bin": { "example-cli": "native/linux-x64/example-cli" } },
                },
            ],
        },
    }));
    for file in ["bin/example-cli.js", "lib/cli.js", "native/linux-x64/example-cli"] {
        touch(dir.path(), file, "#!/usr/bin/env node\n");
    }
    let packed = pack(&opts, &[]).unwrap();
    let entries = |index: usize| {
        let mut entries = tarball_entry_names(
            &dir.path()
                .join(&packed[index].result.tarball_path),
        );
        entries.sort();
        entries
    };
    assert_eq!(
        entries(0),
        ["package/bin/example-cli.js", "package/lib/cli.js", "package/package.json"],
    );
    assert_eq!(entries(1), ["package/native/linux-x64/example-cli", "package/package.json"]);
}

#[test]
fn an_archive_whose_entry_point_is_missing_is_refused() {
    let (dir, opts) = fixture(&addon());
    touch(dir.path(), "index.js", "index");
    touch(dir.path(), "addon.linux-x64-musl.node", "musl");
    match pack(&opts, &["linux-x64-musl"]) {
        Err(PackError::VariantEntryPointMissing { targets, path }) => {
            assert_eq!((targets.as_str(), path.as_str()), ("linux-x64-musl", "musl.js"));
        }
        other => panic!("{other:?}"),
    }
}

/// A variant's bin gets the shebang check a package without variants gets,
/// in a dry run too.
#[test]
fn a_variant_bin_with_a_crlf_shebang_is_refused() {
    let mut manifest = addon();
    manifest["publishConfig"]["variants"][1]["manifest"] =
        json!({ "bin": { "addon": "addon.linux-x64.node" } });
    let (dir, mut opts) = fixture(&manifest);
    write_addon(dir.path());
    touch(dir.path(), "addon.linux-x64.node", "#!/usr/bin/env node\r\n");
    assert!(matches!(pack(&opts, &["linux-x64"]), Err(PackError::BinCrlf { .. })));
    opts.output.dry_run = true;
    assert!(matches!(pack(&opts, &["linux-x64"]), Err(PackError::BinCrlf { .. })));
}

/// An entry's patterns are one `files` list: an exclusion applies to the
/// others, and is not a pattern that matches nothing.
#[test]
fn an_exclusion_applies_to_the_other_patterns_of_its_entry() {
    let (dir, opts) = fixture(&json!({
        "name": "example-cli",
        "version": "4.0.0",
        "files": [],
        "publishConfig": {
            "variants": [{ "targets": ["linux-x64"], "files": ["native/**", "!native/debug/**"] }],
        },
    }));
    touch(dir.path(), "native/cli", "binary");
    touch(dir.path(), "native/debug/cli.pdb", "symbols");
    let packed = pack(&opts, &[]).unwrap();
    let mut entries = tarball_entry_names(
        &dir.path()
            .join(&packed[0].result.tarball_path),
    );
    entries.sort();
    assert_eq!(entries, ["package/native/cli", "package/package.json"]);
}

/// An entry point is a file the archive holds, named by its exact path:
/// never a file that only starts with its path, a path an extension would
/// complete, or a directory.
#[test]
fn an_entry_point_names_a_file_exactly() {
    let missing = |entry_manifest: Value, pattern: &str, files: &[&str]| {
        let (dir, opts) = fixture(&json!({
            "name": "example-cli",
            "version": "4.0.0",
            "files": [],
            "publishConfig": {
                "variants": [{
                    "targets": ["linux-x64"],
                    "files": [pattern],
                    "manifest": entry_manifest,
                }],
            },
        }));
        for file in files {
            touch(dir.path(), file, "content");
        }
        match pack(&opts, &[]) {
            Err(PackError::VariantEntryPointMissing { path, .. }) => Some(path),
            Ok(_) => None,
            Err(other) => panic!("{other:?}"),
        }
    };
    let main = json!({ "main": "dist/index.js" });
    assert_eq!(missing(main.clone(), "dist/**", &["dist/index.js"]), None);
    assert_eq!(missing(main, "dist/**", &["dist/index.js.map"]).as_deref(), Some("dist/index.js"));
    assert_eq!(
        missing(json!({ "main": "dist/index" }), "dist/**", &["dist/index.js"]).as_deref(),
        Some("dist/index"),
    );
    assert_eq!(
        missing(json!({ "main": "dist" }), "dist/**", &["dist/index.js"]).as_deref(),
        Some("dist"),
    );
    let bin = json!({ "bin": { "cli": "native/cli" } });
    assert_eq!(missing(bin.clone(), "native/**", &["native/cli"]), None);
    assert_eq!(missing(bin, "native/**", &["native/cli.debug"]).as_deref(), Some("native/cli"));
}

/// Files no entry claims go into every archive; a file another entry claims
/// stays out of the archive `--variant` packs, even when that entry is not
/// packed, so a build job's archive never carries another platform's files.
#[test]
fn a_targeted_archive_keeps_the_shared_files_but_not_other_entries_claims() {
    let (dir, opts) = fixture(&addon());
    write_addon(dir.path());
    let packed = pack(&opts, &["linux-x64"]).unwrap();
    let mut entries = tarball_entry_names(
        &dir.path()
            .join(&packed[0].result.tarball_path),
    );
    entries.sort();
    assert_eq!(
        entries,
        ["package/addon.linux-x64.node", "package/index.js", "package/package.json"],
        "index.js is shared; lib/ and the musl addon belong to entries not packed",
    );
}

/// The specifiers of an entry's dependencies are replaced as the base
/// manifest's are.
#[test]
fn a_catalog_dependency_of_an_entry_is_replaced() {
    let mut manifest = addon();
    manifest["publishConfig"]["variants"][1]["manifest"] =
        json!({ "dependencies": { "helper": "catalog:" } });
    let (dir, mut opts) = fixture(&manifest);
    opts.manifest.catalogs = std::collections::BTreeMap::from([(
        "default".to_string(),
        std::collections::BTreeMap::from([("helper".to_string(), "^1.2.0".to_string())]),
    )]);
    write_addon(dir.path());
    let packed = pack(&opts, &["linux-x64"]).unwrap();
    let variant: Value = serde_json::from_str(
        &tarball_entry_content(
            &dir.path()
                .join(&packed[0].result.tarball_path),
            "package/package.json",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(variant["dependencies"], json!({ "helper": "^1.2.0" }));
}

/// An archive bundles what its own manifest bundles: a native entry that
/// drops the base manifest's bundled fallback does not ship it.
#[test]
fn an_archive_bundles_what_its_own_manifest_bundles() {
    let (dir, mut opts) = fixture(&json!({
        "name": "example-cli",
        "version": "4.0.0",
        "files": [],
        "dependencies": { "fallback-runtime": "1.0.0" },
        "bundleDependencies": ["fallback-runtime"],
        "publishConfig": {
            "variants": [
                { "targets": ["any"] },
                {
                    "targets": ["linux-x64"],
                    "files": ["native/cli"],
                    "manifest": { "bundleDependencies": null, "dependencies": null },
                },
            ],
        },
    }));
    opts.manifest.node_linker = super::NodeLinker::Hoisted;
    super::install_module(dir.path(), "fallback-runtime", "1.0.0", &[("index.js", "")]);
    touch(dir.path(), "native/cli", "binary");
    let packed = pack(&opts, &[]).unwrap();
    let bundles = |index: usize| {
        packed[index].result.contents
            .iter()
            .any(|path| path.starts_with("node_modules/fallback-runtime/"))
    };
    assert!(bundles(0), "the any archive bundles the fallback");
    assert!(!bundles(1), "the native archive does not");
}

/// An injected file, such as a composed changelog, supersedes the file an
/// entry claims at the same path.
#[test]
fn an_injected_file_supersedes_a_claimed_one() {
    let (dir, mut opts) = fixture(&json!({
        "name": "example-cli",
        "version": "4.0.0",
        "files": [],
        "publishConfig": { "variants": [{ "targets": ["any"], "files": ["CHANGELOG.md"] }] },
    }));
    touch(dir.path(), "CHANGELOG.md", "stale");
    opts.output.injected_files = vec![("package/CHANGELOG.md".to_string(), b"composed".to_vec())];
    let packed = pack(&opts, &[]).unwrap();
    let tarball = dir
        .path()
        .join(&packed[0].result.tarball_path);
    let names = tarball_entry_names(&tarball);
    assert_eq!(
        names
            .iter()
            .filter(|name| *name == "package/CHANGELOG.md")
            .count(),
        1,
    );
    assert_eq!(
        tarball_entry_content(&tarball, "package/CHANGELOG.md").as_deref(),
        Some("composed"),
    );
}

/// An injected file is part of the archive the checks see: it may be the
/// file an entry point names.
#[test]
fn an_injected_file_can_be_an_entry_point() {
    let (_dir, mut opts) = fixture(&json!({
        "name": "example-cli",
        "version": "4.0.0",
        "files": [],
        "publishConfig": {
            "variants": [{ "targets": ["any"], "manifest": { "main": "generated.js" } }],
        },
    }));
    opts.output.injected_files =
        vec![("package/generated.js".to_string(), b"module.exports = 1".to_vec())];
    assert!(pack(&opts, &[]).is_ok());
}

/// With the pnp node linker, an entry cannot bundle dependencies, as a
/// package without variants cannot.
#[test]
fn an_entry_that_bundles_is_refused_with_pnp() {
    let mut manifest = addon();
    manifest["publishConfig"]["variants"][1]["manifest"] =
        json!({ "bundleDependencies": ["helper"] });
    let (dir, mut opts) = fixture(&manifest);
    opts.manifest.node_linker = super::NodeLinker::Pnp;
    write_addon(dir.path());
    assert!(matches!(
        pack(&opts, &["linux-x64"]),
        Err(PackError::BundledDependenciesWithPnp { .. }),
    ));
}
