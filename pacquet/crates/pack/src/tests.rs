use super::{
    Host, PackError, PackOptions, PackResult, api, format_pack_output, to_pack_result_json,
};
use crate::capabilities::{FsCreateDirAll, FsFileLen, FsReadFile, FsWrite};
use flate2::read::GzDecoder;
use pacquet_config::NodeLinker;
use pacquet_reporter::SilentReporter;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    io,
    path::Path,
};
use tempfile::{TempDir, tempdir};

/// Minimal single-project fixture: a `package.json` plus whatever extra
/// files the caller writes. `ignore_scripts` defaults to `true` so the
/// happy-path tests don't shell out to `node`.
fn fixture(manifest: &Value) -> (TempDir, PackOptions) {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();
    let opts = PackOptions {
        dir: dir.path().to_path_buf(),
        catalogs: BTreeMap::new(),
        ignore_scripts: true,
        embed_readme: false,
        pack_gzip_level: None,
        node_linker: NodeLinker::Isolated,
        skip_manifest_obfuscation: false,
        user_agent: "pacquet".to_string(),
        extra_bin_paths: Vec::new(),
        extra_env: HashMap::new(),
        workspace_dir: None,
        dry_run: false,
        pack_destination: None,
        out: None,
    };
    (dir, opts)
}

fn touch(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Read the entry names out of a written `.tgz` for assertions.
fn tarball_entry_names(tarball: &Path) -> Vec<String> {
    let file = std::fs::File::open(tarball).unwrap();
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    archive
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn packs_a_basic_package_to_a_tarball() {
    let (dir, opts) = fixture(&json!({ "name": "foo", "version": "1.2.3" }));
    touch(dir.path(), "index.js", "module.exports = 1\n");

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert_eq!(result.tarball_path, "foo-1.2.3.tgz");
    assert_eq!(result.contents, vec!["index.js".to_string(), "package.json".into()]);

    let tarball = dir.path().join("foo-1.2.3.tgz");
    assert!(tarball.is_file(), "tarball should be written to the project dir");
    let mut names = tarball_entry_names(&tarball);
    names.sort();
    assert_eq!(names, vec!["package/index.js".to_string(), "package/package.json".into()]);
}

#[test]
fn scoped_name_normalizes_the_tarball_filename() {
    let (dir, opts) = fixture(&json!({ "name": "@scope/foo", "version": "0.1.0" }));
    let result = api::<SilentReporter, Host>(&opts).unwrap();
    assert_eq!(result.tarball_path, "scope-foo-0.1.0.tgz");
    assert!(dir.path().join("scope-foo-0.1.0.tgz").is_file());
}

#[test]
fn build_metadata_is_stripped_from_the_packed_version() {
    let (dir, opts) = fixture(&json!({ "name": "foo", "version": "1.0.0+build.7" }));
    let result = api::<SilentReporter, Host>(&opts).unwrap();
    assert_eq!(result.published_manifest["version"], json!("1.0.0"));
    assert_eq!(result.tarball_path, "foo-1.0.0.tgz");
    assert!(dir.path().join("foo-1.0.0.tgz").is_file());
}

#[test]
fn manifest_entry_is_the_published_manifest_not_the_on_disk_one() {
    let (dir, opts) = fixture(&json!({
        "name": "foo",
        "version": "1.0.0",
        "pnpm": { "overrides": {} },
        "scripts": { "prepack": "tsc" },
    }));

    api::<SilentReporter, Host>(&opts).unwrap();

    // The obfuscated `pnpm` field and the publish-lifecycle `prepack`
    // script must be absent from the manifest packed inside the tarball.
    let tarball = dir.path().join("foo-1.0.0.tgz");
    let file = std::fs::File::open(&tarball).unwrap();
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut packed_manifest = None;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap().to_string_lossy() == "package/package.json" {
            let mut buf = String::new();
            io::Read::read_to_string(&mut entry, &mut buf).unwrap();
            packed_manifest = Some(serde_json::from_str::<Value>(&buf).unwrap());
        }
    }
    let packed = packed_manifest.expect("tarball carries package/package.json");
    assert!(packed.get("pnpm").is_none());
    assert!(packed.get("scripts").and_then(|s| s.get("prepack")).is_none());
    assert_eq!(packed["version"], json!("1.0.0"));
}

#[test]
fn dry_run_reports_without_writing_a_tarball() {
    let (dir, mut opts) = fixture(&json!({ "name": "foo", "version": "1.0.0" }));
    touch(dir.path(), "index.js", "x\n");
    opts.dry_run = true;

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert!(!dir.path().join("foo-1.0.0.tgz").exists(), "dry run must not write a tarball");
    assert_eq!(result.contents, vec!["index.js".to_string(), "package.json".into()]);
    assert!(result.unpacked_size > 0);
}

#[test]
fn unpacked_size_sums_file_and_manifest_bytes() {
    let (dir, opts) = fixture(&json!({ "name": "foo", "version": "1.0.0" }));
    touch(dir.path(), "index.js", "0123456789"); // 10 bytes

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    let manifest_len = serde_json::to_string_pretty(&result.published_manifest).unwrap().len();
    assert_eq!(result.unpacked_size, manifest_len as u64 + 10);
}

#[test]
fn files_field_restricts_the_tarball_contents() {
    let (dir, opts) = fixture(&json!({
        "name": "foo",
        "version": "1.0.0",
        "files": ["dist"],
    }));
    touch(dir.path(), "dist/index.js", "x\n");
    touch(dir.path(), "src/index.ts", "x\n");

    let result = api::<SilentReporter, Host>(&opts).unwrap();
    assert_eq!(result.contents, vec!["dist/index.js".to_string(), "package.json".into()]);
}

#[test]
fn missing_name_is_rejected() {
    let (_dir, opts) = fixture(&json!({ "version": "1.0.0" }));
    assert!(matches!(api::<SilentReporter, Host>(&opts), Err(PackError::PackageNameNotFound)));
}

#[test]
fn missing_version_is_rejected() {
    let (_dir, opts) = fixture(&json!({ "name": "foo" }));
    assert!(matches!(api::<SilentReporter, Host>(&opts), Err(PackError::PackageVersionNotFound)));
}

#[test]
fn invalid_package_name_is_rejected() {
    let (_dir, opts) = fixture(&json!({ "name": "Foo BAR", "version": "1.0.0" }));
    assert!(matches!(
        api::<SilentReporter, Host>(&opts),
        Err(PackError::InvalidPackageName { .. })
    ));
}

#[test]
fn out_and_pack_destination_together_is_rejected() {
    let (_dir, mut opts) = fixture(&json!({ "name": "foo", "version": "1.0.0" }));
    opts.out = Some("%s.tgz".to_string());
    opts.pack_destination = Some("dest".to_string());
    assert!(matches!(api::<SilentReporter, Host>(&opts), Err(PackError::OutAndPackDestination)));
}

#[test]
fn bundled_dependencies_without_hoisted_is_rejected() {
    let (_dir, opts) = fixture(&json!({
        "name": "foo",
        "version": "1.0.0",
        "bundledDependencies": ["bar"],
    }));
    assert!(matches!(
        api::<SilentReporter, Host>(&opts),
        Err(PackError::BundledDependenciesWithoutHoisted { field: "bundledDependencies", .. })
    ));
}

#[test]
fn out_template_substitutes_name_and_version_and_directory() {
    let (dir, mut opts) = fixture(&json!({ "name": "@scope/foo", "version": "2.0.0" }));
    opts.out = Some("artifacts/%s-%v.tgz".to_string());

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    let expected = dir.path().join("artifacts").join("scope-foo-2.0.0.tgz");
    assert!(expected.is_file(), "tarball should land in the templated directory");
    assert_eq!(result.tarball_path, expected.display().to_string());
}

#[test]
fn workspace_license_is_injected_into_a_sub_package() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("LICENSE"), "MIT").unwrap();
    let pkg_dir = workspace.path().join("packages").join("foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("package.json"),
        serde_json::to_string(&json!({ "name": "foo", "version": "1.0.0" })).unwrap(),
    )
    .unwrap();

    let opts = PackOptions {
        dir: pkg_dir.clone(),
        catalogs: BTreeMap::new(),
        ignore_scripts: true,
        embed_readme: false,
        pack_gzip_level: None,
        node_linker: NodeLinker::Isolated,
        skip_manifest_obfuscation: false,
        user_agent: "pacquet".to_string(),
        extra_bin_paths: Vec::new(),
        extra_env: HashMap::new(),
        workspace_dir: Some(workspace.path().to_path_buf()),
        dry_run: false,
        pack_destination: None,
        out: None,
    };

    let result = api::<SilentReporter, Host>(&opts).unwrap();
    assert!(result.contents.contains(&"LICENSE".to_string()));
    let names = tarball_entry_names(&pkg_dir.join("foo-1.0.0.tgz"));
    assert!(names.contains(&"package/LICENSE".to_string()));
}

/// The write-phase DI seam: a fake whose `FsWrite` fails with
/// `PermissionDenied` must surface as `PackError::WriteTarball`. This is
/// the branch a real fixture can't reach portably, justifying the `Sys`
/// generic.
#[test]
fn tarball_write_failure_surfaces_as_write_error() {
    struct DeniedWrite;
    impl FsReadFile for DeniedWrite {
        fn read_file(path: &Path) -> io::Result<Vec<u8>> {
            std::fs::read(path)
        }
    }
    impl FsFileLen for DeniedWrite {
        fn file_len(path: &Path) -> io::Result<u64> {
            std::fs::metadata(path).map(|metadata| metadata.len())
        }
    }
    impl FsCreateDirAll for DeniedWrite {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for DeniedWrite {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "mocked"))
        }
    }

    let (_dir, opts) = fixture(&json!({ "name": "foo", "version": "1.0.0" }));
    let err = api::<SilentReporter, DeniedWrite>(&opts).unwrap_err();
    assert!(matches!(err, PackError::WriteTarball { .. }), "got {err:?}");
}

#[test]
fn format_pack_output_json_single_vs_multiple() {
    let result = PackResult {
        published_manifest: json!({ "name": "foo", "version": "1.0.0" }),
        contents: vec!["package.json".to_string()],
        tarball_path: "foo-1.0.0.tgz".to_string(),
        unpacked_size: 42,
    };
    let json_single = format_pack_output(&[to_pack_result_json(&result)], true, false);
    let parsed: Value = serde_json::from_str(&json_single).unwrap();
    assert_eq!(parsed["name"], json!("foo"));
    assert_eq!(parsed["filename"], json!("foo-1.0.0.tgz"));
    assert_eq!(parsed["files"], json!([{ "path": "package.json" }]));

    let two = vec![to_pack_result_json(&result), to_pack_result_json(&result)];
    let json_multi = format_pack_output(&two, true, false);
    let parsed_multi: Value = serde_json::from_str(&json_multi).unwrap();
    assert!(parsed_multi.is_array());
}

#[test]
fn format_pack_output_text_block() {
    let result = PackResult {
        published_manifest: json!({ "name": "foo", "version": "1.0.0" }),
        contents: vec!["index.js".to_string(), "package.json".into()],
        tarball_path: "foo-1.0.0.tgz".to_string(),
        unpacked_size: 0,
    };
    let text = format_pack_output(&[to_pack_result_json(&result)], false, false);
    assert!(text.starts_with("package: foo@1.0.0"));
    assert!(text.contains("Tarball Contents"));
    assert!(text.contains("index.js\npackage.json"));
    assert!(text.contains("Tarball Details"));
    assert!(text.contains("foo-1.0.0.tgz"));
}

/// Port of pnpm's `pack: runs prepack, prepare, and postpack`
/// ([pack.ts](https://github.com/pnpm/pnpm/blob/cab1c11c69/releasing/commands/test/publish/pack.ts#L346-L368)).
/// Exercises `run_scripts_if_present` / `script_body`: with scripts
/// enabled, each of `prepack` / `prepare` / `postpack` runs and leaves a
/// marker file. Gated to Unix like the executor's other script-running
/// tests, since it shells out through the platform shell.
#[cfg(unix)]
#[test]
fn runs_prepack_prepare_and_postpack() {
    let (dir, mut opts) = fixture(&json!({
        "name": "foo",
        "version": "1.0.0",
        "scripts": {
            "prepack": "touch prepack.ran",
            "prepare": "touch prepare.ran",
            "postpack": "touch postpack.ran",
        },
    }));
    opts.ignore_scripts = false;

    api::<SilentReporter, Host>(&opts).unwrap();

    assert!(dir.path().join("foo-1.0.0.tgz").is_file());
    assert!(dir.path().join("prepack.ran").exists(), "prepack should have run");
    assert!(dir.path().join("prepare.ran").exists(), "prepare should have run");
    assert!(dir.path().join("postpack.ran").exists(), "postpack should have run");
}

/// With scripts enabled but the manifest declaring none of the
/// requested lifecycle scripts (here an empty `prepack`), `script_body`
/// yields `None` and `run_scripts_if_present` returns early, so packing
/// still succeeds without shelling out.
#[test]
fn pack_succeeds_when_scripts_enabled_but_absent() {
    let (dir, mut opts) = fixture(&json!({
        "name": "foo",
        "version": "1.0.0",
        "scripts": { "prepack": "" },
    }));
    opts.ignore_scripts = false;

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert_eq!(result.tarball_path, "foo-1.0.0.tgz");
    assert!(dir.path().join("foo-1.0.0.tgz").is_file());
}

/// Write a package under `<dir>/node_modules/<name>/` for the bundle
/// tests.
fn install_module(dir: &Path, name: &str, version: &str, extra: &[(&str, &str)]) {
    let module = dir.join("node_modules").join(name);
    std::fs::create_dir_all(&module).unwrap();
    std::fs::write(
        module.join("package.json"),
        serde_json::to_string(&json!({ "name": name, "version": version })).unwrap(),
    )
    .unwrap();
    for (rel, contents) in extra {
        std::fs::write(module.join(rel), contents).unwrap();
    }
}

/// Port of pnpm's `pack: bundles dependencies listed in bundleDependencies`
/// ([pack.ts](https://github.com/pnpm/pnpm/blob/cab1c11c69/releasing/commands/test/publish/pack.ts#L102-L128)).
/// Covers the `fs-packlist` `bundleDependencies` recursion and the
/// `node_linker: hoisted` allow-path.
#[test]
fn bundles_dependencies_listed_in_bundle_dependencies() {
    let (dir, mut opts) = fixture(&json!({
        "name": "pkg-with-bundle-deps",
        "version": "0.0.0",
        "bundleDependencies": ["bundled-dep"],
    }));
    opts.node_linker = NodeLinker::Hoisted;
    install_module(dir.path(), "bundled-dep", "1.0.0", &[("index.js", "module.exports = 42")]);
    install_module(dir.path(), "not-bundled", "1.0.0", &[]);

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert!(result.contents.contains(&"node_modules/bundled-dep/package.json".to_string()));
    assert!(result.contents.contains(&"node_modules/bundled-dep/index.js".to_string()));
    assert!(!result.contents.iter().any(|path| path.contains("not-bundled")));
}

/// Port of pnpm's `pack: bundles every dependency when bundleDependencies
/// is true`
/// ([pack.ts](https://github.com/pnpm/pnpm/blob/cab1c11c69/releasing/commands/test/publish/pack.ts#L130-L159)).
/// Covers `bundle_dep_names`' `bundleDependencies: true` branch, which
/// materializes the names from `dependencies`.
#[test]
fn bundles_every_dependency_when_bundle_dependencies_is_true() {
    let (dir, mut opts) = fixture(&json!({
        "name": "pkg-with-bundle-deps-true",
        "version": "0.0.0",
        "dependencies": { "bundled-dep": "1.0.0" },
        "bundleDependencies": true,
    }));
    opts.node_linker = NodeLinker::Hoisted;
    install_module(dir.path(), "bundled-dep", "1.0.0", &[("index.js", "module.exports = 42")]);
    install_module(dir.path(), "not-a-dep", "1.0.0", &[]);

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert!(result.contents.contains(&"node_modules/bundled-dep/index.js".to_string()));
    assert!(result.contents.contains(&"node_modules/bundled-dep/package.json".to_string()));
    assert!(!result.contents.iter().any(|path| path.contains("not-a-dep")));
}

/// Port of pnpm's `pack should read from the correct node_modules when
/// publishing from a custom directory`
/// ([pack.ts](https://github.com/pnpm/pnpm/blob/cab1c11c69/releasing/commands/test/publish/pack.ts#L503-L541)).
/// Covers the `publishConfig.directory` redirect: the manifest is read
/// from `dist/`, but `workspace:` deps still resolve against the project
/// root's `node_modules`.
#[test]
fn reads_node_modules_from_original_dir_when_publishing_from_custom_directory() {
    let (dir, opts) = fixture(&json!({
        "name": "custom-publish-dir",
        "version": "0.0.0",
        "publishConfig": { "directory": "dist" },
        "dependencies": { "local": "workspace:*" },
    }));
    let dist = dir.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::copy(dir.path().join("package.json"), dist.join("package.json")).unwrap();
    install_module(dir.path(), "local", "1.0.0", &[]);

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    // The `workspace:*` spec resolves to the installed version, read from
    // the project root's node_modules (not `dist/node_modules`).
    assert_eq!(result.published_manifest["dependencies"]["local"], json!("1.0.0"));
}
