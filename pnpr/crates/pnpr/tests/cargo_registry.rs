//! The Cargo registry surface: sparse index, downloads, `cargo publish`,
//! yank, and proxying an upstream sparse index and its downloads.

#[path = "cargo_registry/behavior.rs"]
mod behavior;

#[path = "cargo_registry/publishing.rs"]
mod publishing;

#[path = "cargo_registry/routing.rs"]
mod routing;

use crate::{ecosystem as common, registry_groups};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::{HostedSource, body_bytes, find_file, mixed_router_config, sha256_hex};
use pnpr::{AuthState, Config, Ecosystem, router_with_auth};
use serde_json::{Value, json};
use std::{io::Write as _, path::PathBuf};
use tempfile::TempDir;
use tower::ServiceExt;

/// A hosted Cargo registry (`crates`, claiming `demo` and `inflector`) and
/// a Cargo upstream (`cratesio`, everything else) at `upstream_url`, both in
/// the `main` router beside the npm registries.
fn cargo_config(storage: PathBuf, upstream_url: &str, hosted_access: &str) -> Config {
    mixed_router_config(
        storage,
        Ecosystem::Cargo,
        HostedSource {
            name: "crates",
            org: "crates",
            access: hosted_access,
            packages: &["demo", "inflector"],
        },
        ("cratesio", upstream_url),
    )
}

fn crate_archive(name: &str, version: &str) -> Vec<u8> {
    let root = format!("{name}-{version}");
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, contents) in [
        ("Cargo.toml", format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")),
        ("src/lib.rs", "pub fn demo() {}\n".to_string()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("{root}/{path}"), contents.as_bytes())
            .unwrap();
    }
    builder
        .into_inner()
        .unwrap()
        .finish()
        .unwrap()
}

fn publish_body(metadata: &Value, archive: &[u8]) -> Vec<u8> {
    let metadata = serde_json::to_vec(metadata).unwrap();
    let mut body = Vec::new();
    body.write_all(&(metadata.len() as u32).to_le_bytes())
        .unwrap();
    body.write_all(&metadata).unwrap();
    body.write_all(&(archive.len() as u32).to_le_bytes())
        .unwrap();
    body.write_all(archive).unwrap();
    body
}

fn metadata(name: &str, version: &str) -> Value {
    json!({
        "name": name,
        "vers": version,
        "deps": [{
            "name": "serde",
            "version_req": "^1",
            "features": ["derive"],
            "optional": false,
            "default_features": true,
            "target": null,
            "kind": "normal",
            "registry": null,
            "explicit_name_in_toml": null,
        }],
        "features": {},
        "authors": ["someone"],
        "description": "A demo crate",
        "documentation": null,
        "homepage": null,
        "readme": null,
        "readme_file": null,
        "keywords": [],
        "categories": [],
        "license": "MIT",
        "license_file": null,
        "repository": null,
        "badges": {},
        "links": null,
        "rust_version": null,
    })
}

/// `cargo` sends a registry token as the bare header value, with no scheme.
fn publish_request(token: Option<&str>, body: Vec<u8>) -> Request<Body> {
    let mut request = Request::put("/cargo/api/v1/crates/new");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, token);
    }
    request.body(Body::from(body)).unwrap()
}
