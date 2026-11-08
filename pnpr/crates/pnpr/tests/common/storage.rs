//! Builds a hosted `storage` directory in a `TempDir` for the static-serve
//! tests. The packuments carry the rich publish metadata a real
//! verdaccio/npm registry emits (`_attachments`, `_uplinks`, `_distfiles`,
//! `users`, per-version `_nodeVersion`, `_id`, `contributors`) so the tests can
//! assert that pnpr rewrites tarball URLs and strips those fields in
//! the abbreviated packument form. No real tarball is needed — the bytes are
//! arbitrary; nothing re-hashes them.

use std::path::Path;

use pnpr_storage::layout::write_local_layout_marker;
use serde_json::{Value, json};
use tempfile::TempDir;

// Only asserted for pass-through / `sha512-` prefix, never recomputed.
const SHASUM: &str = "a1c3e0c08af5ec17f150b8b9f067bead3d64e472";
const INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
const TIMESTAMP: &str = "2020-01-01T00:00:00.000Z";

/// Create a temp storage dir holding `@foo/no-deps` (rich verdaccio metadata)
/// and `@pnpm.e2e/needs-auth` (used by the auth-policy tests).
pub fn build_storage() -> TempDir {
    let dir = TempDir::new().expect("create temp storage dir");
    write_local_layout_marker(dir.path()).expect("write layout marker");
    write_package(dir.path(), "@foo/no-deps", "no-deps-1.0.0.tgz", no_deps_packument());
    write_package(
        dir.path(),
        "@pnpm.e2e/needs-auth",
        "needs-auth-1.0.0.tgz",
        needs_auth_packument(),
    );
    dir
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn write_package(storage: &Path, name: &str, tarball: &str, packument: Value) {
    let npm = pnpr_storage::Ecosystem::Npm;
    pnpr_fixtures::write_hosted_document(
        storage,
        "",
        npm,
        name,
        &serde_json::to_vec(&packument).unwrap(),
    );
    let bytes = format!("fake {name} tarball");
    pnpr_fixtures::write_hosted_file(
        storage,
        ("", npm),
        (name, "1.0.0"),
        tarball,
        bytes.as_bytes(),
    );
}

/// The bytes the hosted store at `storage` serves for the file `file` of
/// npm package `name` in the default namespace, `None` when it maps none.
#[must_use]
pub fn hosted_file(storage: &Path, name: &str, file: &str) -> Option<Vec<u8>> {
    pnpr_fixtures::read_hosted_file(storage, ("", pnpr_storage::Ecosystem::Npm), name, file)
}

/// The names stored where the hosted store at `storage` keeps the archives
/// of `(org, ecosystem)`, keys pnpr stored and anything else.
#[must_use]
pub fn stored_archives(
    storage: &Path,
    org: &str,
    ecosystem: pnpr_storage::Ecosystem,
) -> Vec<String> {
    let mut dir = storage.to_path_buf();
    dir.extend(pnpr_storage::layout::namespace_segments(org, ecosystem));
    dir.push("archives");
    let mut names: Vec<String> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.depth() == 2 && entry.file_type().is_file())
        .map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn no_deps_packument() -> Value {
    json!({
        "name": "@foo/no-deps",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@foo/no-deps",
                "version": "1.0.0",
                "_id": "@foo/no-deps@1.0.0",
                "_nodeVersion": "25.6.1",
                "contributors": [],
                "dist": {
                    // Verdaccio form (scope repeated in the filename); the
                    // server must rewrite this to the public-url npm form.
                    "tarball": "http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz",
                    "shasum": SHASUM,
                    "integrity": INTEGRITY,
                },
            },
        },
        "time": { "created": TIMESTAMP, "modified": TIMESTAMP, "1.0.0": TIMESTAMP },
        "users": { "someone": true },
        "_uplinks": {},
        "_distfiles": {},
        "_attachments": {
            "@foo/no-deps-1.0.0.tgz": { "content_type": "application/octet-stream", "length": 0 },
        },
        "_rev": "1-0",
        "_id": "@foo/no-deps",
        "readme": "# no-deps",
    })
}

fn needs_auth_packument() -> Value {
    json!({
        "name": "@pnpm.e2e/needs-auth",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@pnpm.e2e/needs-auth",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:4873/@pnpm.e2e/needs-auth/-/@pnpm.e2e/needs-auth-1.0.0.tgz",
                    "shasum": SHASUM,
                    "integrity": INTEGRITY,
                },
            },
        },
        "time": { "created": TIMESTAMP, "modified": TIMESTAMP, "1.0.0": TIMESTAMP },
    })
}

/// Every file below the `(org, ecosystem)` namespace of the hosted store at
/// `storage`: its stored archives, and anything a publish staged there and
/// left behind.
#[must_use]
pub fn namespace_files(
    storage: &Path,
    org: &str,
    ecosystem: pnpr_storage::Ecosystem,
) -> Vec<std::path::PathBuf> {
    let mut dir = storage.to_path_buf();
    dir.extend(pnpr_storage::layout::namespace_segments(org, ecosystem));
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect()
}

/// The bytes the hosted store `config` describes serves for the file `file`
/// of `name` in the `ecosystem` namespace of `org`, through the metadata
/// database it names, `None` when the package maps no such file.
pub async fn served_file(
    config: &pnpr::Config,
    (org, ecosystem): (&str, pnpr_storage::Ecosystem),
    name: &str,
    file: &str,
) -> Option<Vec<u8>> {
    let storage =
        pnpr_storage::Storage::from_config(&config.storage).unwrap().for_hosted(org, ecosystem);
    let name = pnpr_package_name::CanonicalPackageName::parse(name, ecosystem).unwrap();
    let (body, _) = storage.open_hosted_blob(&name, file).await.unwrap()?;
    Some(axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec())
}
