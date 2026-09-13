//! Builds a verdaccio-shaped `storage` directory in a `TempDir` for the
//! static-serve tests. The packuments carry the rich publish metadata a real
//! verdaccio/npm registry emits (`_attachments`, `_uplinks`, `_distfiles`,
//! `users`, per-version `_nodeVersion`, `_id`, `contributors`) so the tests can
//! assert that pnpr rewrites tarball URLs and strips those fields in
//! the abbreviated packument form. No real tarball is needed — the bytes are
//! arbitrary; nothing re-hashes them.

use std::path::Path;

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
    let package_dir = storage.join(name);
    std::fs::create_dir_all(&package_dir).expect("create package dir");
    std::fs::write(package_dir.join("package.json"), serde_json::to_vec(&packument).unwrap())
        .expect("write packument");
    std::fs::write(package_dir.join(tarball), format!("fake {name} tarball"))
        .expect("write tarball");
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
