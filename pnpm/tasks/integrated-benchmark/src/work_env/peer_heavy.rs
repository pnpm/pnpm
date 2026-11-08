//! The peer-heavy scenario's registry packages: one shared peer provider and
//! [`PEER_HEAVY_DEPTH`] layers of [`PEER_HEAVY_WIDTH`] packages, each
//! depending on the whole next layer and peering on the provider.
//!
//! The shared mock gets them committed into its metadata database. A
//! revision's own mock gets them published over HTTP by that revision's
//! `pnpr`, so a revision with another metadata schema serves them too. Both
//! carry the same archives, so every target resolves the same lockfile.

use super::{
    PEER_HEAVY_DEPTH, PEER_HEAVY_PROVIDER, PEER_HEAVY_VERSION, PEER_HEAVY_WIDTH, WorkEnv,
    fixtures::peer_heavy_package_name, mint_pnpr_token, serves_npm_under_base_path,
};
use crate::cli_args::BenchmarkScenario;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Map, Value, json};
use std::path::Path;

const PUBLISHED_AT: &str = "2020-01-01T00:00:00.000Z";

/// Commit the peer-heavy packages to the registry metadata database at
/// `database`. Their archives are never fetched, so none is written; their
/// integrity is the one a publish over HTTP stores.
pub(crate) fn seed_peer_heavy_registry(database: &Path) {
    let packuments = peer_heavy_packages()
        .into_iter()
        .map(|(name, manifest)| {
            let packument = packument(&name, &manifest);
            (name, packument)
        })
        .collect();
    pnpr_fixtures::write_packuments_into(database, packuments);
}

/// Publish the peer-heavy packages to the npm registry at `registry` as the
/// caller `token` signs in, each with its archive.
pub(super) fn publish_peer_heavy(registry: &str, token: &str) {
    let registry = registry.trim_end_matches('/').to_string();
    let token = token.to_string();
    let published = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create the peer-heavy publish runtime")
            .block_on(publish_all(&registry, &token));
    });
    published.join().expect("the peer-heavy publish thread panicked");
}

async fn publish_all(registry: &str, token: &str) {
    let client = reqwest::Client::new();
    for (name, manifest) in peer_heavy_packages() {
        let url = format!("{registry}/{}", name.replace('/', "%2f"));
        let response = client
            .put(&url)
            .bearer_auth(token)
            .json(&publish_document(&name, manifest))
            .send()
            .await
            .unwrap_or_else(|error| panic!("publish {name} to {url}: {error}"));
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        assert!(status.is_success(), "publishing {name} to {url} returned {status}: {body}");
    }
}

/// Every peer-heavy package with its version's manifest, `dist` included.
pub(super) fn peer_heavy_packages() -> Vec<(String, Value)> {
    let provider = manifest(PEER_HEAVY_PROVIDER, &Map::new(), false);
    let mut packages = vec![(PEER_HEAVY_PROVIDER.to_string(), provider)];
    for level in 0..PEER_HEAVY_DEPTH {
        let dependencies: Map<String, Value> = if level + 1 == PEER_HEAVY_DEPTH {
            Map::new()
        } else {
            (0..PEER_HEAVY_WIDTH)
                .map(|index| (peer_heavy_package_name(level + 1, index), json!(PEER_HEAVY_VERSION)))
                .collect()
        };
        for index in 0..PEER_HEAVY_WIDTH {
            let name = peer_heavy_package_name(level, index);
            let manifest = manifest(&name, &dependencies, true);
            packages.push((name, manifest));
        }
    }
    packages
}

/// The packument of `name`, whose one version is `manifest`.
pub(super) fn packument(name: &str, manifest: &Value) -> Value {
    json!({
        "name": name,
        "dist-tags": { "latest": PEER_HEAVY_VERSION },
        "versions": { PEER_HEAVY_VERSION: manifest },
        "time": { "created": PUBLISHED_AT, "modified": PUBLISHED_AT, PEER_HEAVY_VERSION: PUBLISHED_AT },
    })
}

/// The npm publish document of `name`'s version `manifest`, with its
/// archive attached.
fn publish_document(name: &str, mut manifest: Value) -> Value {
    let archive = tarball(&package_json(&manifest));
    let file = archive_file(name);
    manifest["dist"]["tarball"] = json!(format!("http://example.test/{name}/-/{file}"));
    json!({
        "_id": name,
        "name": name,
        "dist-tags": { "latest": PEER_HEAVY_VERSION },
        "versions": { PEER_HEAVY_VERSION: manifest },
        "_attachments": {
            file: {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(&archive),
                "length": archive.len(),
            },
        },
    })
}

/// `name`'s version manifest, with the `dist` of its archive.
pub(super) fn manifest(name: &str, dependencies: &Map<String, Value>, has_peer: bool) -> Value {
    let mut manifest = json!({
        "name": name,
        "version": PEER_HEAVY_VERSION,
        "dependencies": dependencies,
    });
    if has_peer {
        manifest["peerDependencies"] = json!({ PEER_HEAVY_PROVIDER: PEER_HEAVY_VERSION });
    }
    let archive = tarball(&manifest);
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&archive)
        .result();
    manifest["dist"] = json!({
        "tarball": format!("http://example.test/{name}/-/{}", archive_file(name)),
        "integrity": integrity.to_string(),
    });
    manifest
}

/// The manifest an archive's `package.json` holds: `manifest` without its
/// `dist`.
fn package_json(manifest: &Value) -> Value {
    let mut package = manifest.clone();
    if let Some(fields) = package.as_object_mut() {
        fields.remove("dist");
    }
    package
}

/// The archive of a package whose `package.json` is `package`: the same
/// bytes on every build, so its integrity is fixed.
pub(super) fn tarball(package: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec_pretty(package).expect("serialize the peer-heavy manifest");
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    archive
        .append_data(&mut header, "package/package.json", bytes.as_slice())
        .expect("write the peer-heavy archive");
    archive
        .into_inner()
        .and_then(GzEncoder::finish)
        .expect("finish the peer-heavy archive")
}

fn archive_file(name: &str) -> String {
    let basename = name
        .rsplit('/')
        .next()
        .expect("a package name has a final segment");
    format!("{basename}-{PEER_HEAVY_VERSION}.tgz")
}

/// Publish the peer-heavy packages through the revision mock on
/// `mock_port`, so its own `pnpr` stores them in its own metadata database.
fn publish_peer_heavy_to(mock_port: u16) {
    let origin = format!("http://127.0.0.1:{mock_port}");
    let registry = if serves_npm_under_base_path(mock_port) {
        pnpm_registry_mock::npm_registry_url(&origin)
    } else {
        format!("{origin}/")
    };
    eprintln!("Publishing the peer-heavy packages to the revision mock at {registry}...");
    publish_peer_heavy(&registry, &mint_pnpr_token(mock_port));
}

impl WorkEnv {
    fn uses_peer_heavy_fixture(&self) -> bool {
        self.options.selection.scenario.is_some_and(BenchmarkScenario::uses_peer_heavy_fixture)
    }

    /// Publish the scenario's packages through the revision mock on
    /// `mock_port`, once it is up.
    pub(super) fn publish_scenario_packages(&self, mock_port: u16) {
        if self.uses_peer_heavy_fixture() {
            publish_peer_heavy_to(mock_port);
        }
    }
}

#[cfg(test)]
mod tests;
