use super::{build_storage_at_with_substitutions, ensure_storage, latest_version, packages_dir};
use std::{collections::BTreeSet, path::Path};

fn tarball_entries(tarball: &Path) -> BTreeSet<String> {
    let bytes = std::fs::read(tarball).expect("read fixture tarball");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)));
    archive
        .entries()
        .expect("read tar entries")
        .map(|entry| {
            entry.expect("read tar entry").path().expect("tar entry path").display().to_string()
        })
        .collect()
}

fn tarball_package_manifest(tarball: &Path) -> serde_json::Value {
    let bytes = std::fs::read(tarball).expect("read fixture tarball");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)));
    let mut entry = archive
        .entries()
        .expect("read tar entries")
        .find_map(|entry| {
            let entry = entry.expect("read tar entry");
            (entry.path().expect("tar entry path") == Path::new("package/package.json"))
                .then_some(entry)
        })
        .expect("package.json entry");
    serde_json::from_reader(&mut entry).expect("parse package.json entry")
}

#[test]
fn latest_version_uses_semver_prerelease_order() {
    let versions = ["1.0.0-beta.2".to_string(), "1.0.0-beta.10".to_string(), "1.0.0".to_string()];
    assert_eq!(latest_version(versions.iter()), Some("1.0.0".to_string()));
}

#[test]
fn ensure_storage_generates_packuments_and_tarballs() {
    let storage = ensure_storage();
    assert!(packages_dir().join("@pnpm.e2e/abc/1.0.0/package.json").exists());
    assert!(storage.join("@pnpm.e2e/abc/package.json").exists());
    assert!(storage.join("@pnpm.e2e/abc/abc-1.0.0.tgz").exists());
}

#[test]
fn per_run_substitutions_update_packuments_and_tarballs() {
    let out = tempfile::tempdir().expect("create output directory");
    build_storage_at_with_substitutions(
        &packages_dir(),
        out.path(),
        &[(
            "github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd",
            "git+file:///tmp/hi#main",
        )],
    );
    let package_dir = out.path().join("@pnpm.e2e/has-aliased-git-dependency");
    let packument: serde_json::Value = serde_json::from_slice(
        &std::fs::read(package_dir.join("package.json")).expect("read substituted packument"),
    )
    .expect("parse substituted packument");
    assert_eq!(packument["versions"]["1.0.0"]["dependencies"]["say-hi"], "git+file:///tmp/hi#main");
    let manifest =
        tarball_package_manifest(&package_dir.join("has-aliased-git-dependency-1.0.0.tgz"));
    assert_eq!(manifest["dependencies"]["say-hi"], "git+file:///tmp/hi#main");
}

// Both case variants land in the tarball even though a case-insensitive
// working tree cannot hold `Foo.js` and `foo.js` side by side on disk.
#[test]
fn case_colliding_files_are_composed_in_memory() {
    let storage = ensure_storage();
    let entries = tarball_entries(&storage.join(
        "@pnpm.e2e/with-same-file-in-different-cases/with-same-file-in-different-cases-1.0.0.tgz",
    ));
    assert!(entries.contains("package/Foo.js"), "{entries:?}");
    assert!(entries.contains("package/foo.js"), "{entries:?}");
}

// `bundleDependencies` packages embed the resolved dependency in the
// tarball's `node_modules`, reproduced by the builder so the gitignored
// `node_modules` never needs to be committed.
#[test]
fn bundle_dependencies_embed_node_modules() {
    let storage = ensure_storage();
    let bundled = tarball_entries(
        &storage
            .join("@pnpm.e2e/pkg-with-bundle-dependencies/pkg-with-bundle-dependencies-1.0.0.tgz"),
    );
    assert!(
        bundled.contains("package/node_modules/@pnpm.e2e/hello-world-js-bin/package.json"),
        "{bundled:?}",
    );

    let not_bundled = tarball_entries(&storage.join(
        "@pnpm.e2e/pkg-with-bundle-dependencies-false/pkg-with-bundle-dependencies-false-1.0.0.tgz",
    ));
    assert!(
        !not_bundled.iter().any(|entry| entry.contains("node_modules")),
        "bundleDependencies:false must not embed node_modules: {not_bundled:?}",
    );
}

// pnpm publish copies the root LICENSE into each package, except the
// self-contained-workspace fixtures that ship without one.
#[test]
fn root_license_is_injected_except_for_self_contained_workspaces() {
    let storage = ensure_storage();
    let abc = tarball_entries(&storage.join("@pnpm.e2e/abc/abc-1.0.0.tgz"));
    assert!(abc.contains("package/LICENSE"), "{abc:?}");

    let bundled =
        tarball_entries(&storage.join(
            "@pnpm.e2e/pkg-with-bundled-dependencies/pkg-with-bundled-dependencies-1.0.0.tgz",
        ));
    assert!(!bundled.contains("package/LICENSE"), "{bundled:?}");
}
