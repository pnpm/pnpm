use super::{manifest, package_json, peer_heavy_packages, publish_document, tarball};
use crate::work_env::{PEER_HEAVY_DEPTH, PEER_HEAVY_PROVIDER, PEER_HEAVY_WIDTH};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Map, Value, json};
use std::io::Read as _;

/// A package's archive holds its manifest as `package.json`, and is the
/// same bytes on every build, so the integrity its `dist` declares holds for
/// the archive a publish attaches.
#[test]
fn a_peer_heavy_archive_is_reproducible_and_matches_its_integrity() {
    let dependencies = Map::from_iter([("dep".to_string(), json!("1.0.0"))]);
    let manifest = manifest("@pnpmtest/peer-benchmark-level-0-00", &dependencies, true);
    let archive = tarball(&package_json(&manifest));
    assert_eq!(archive, tarball(&package_json(&manifest)));
    let integrity: ssri::Integrity = manifest["dist"]["integrity"]
        .as_str()
        .expect("an integrity")
        .parse()
        .expect("a valid integrity");
    integrity.check(&archive).expect("the archive matches its integrity");

    let decoder = flate2::read::GzDecoder::new(archive.as_slice());
    let mut entries = tar::Archive::new(decoder);
    let mut entry = entries
        .entries()
        .unwrap()
        .next()
        .expect("an entry")
        .unwrap();
    assert_eq!(entry.path().unwrap().to_str(), Some("package/package.json"));
    let mut package = String::new();
    entry.read_to_string(&mut package).unwrap();
    let package: Value = serde_json::from_str(&package).unwrap();
    assert_eq!(package, package_json(&manifest));
    assert!(package.get("dist").is_none());
}

/// A publish document attaches the archive its version's `dist` describes.
#[test]
fn a_peer_heavy_publish_attaches_the_archive_its_dist_describes() {
    let packages = peer_heavy_packages();
    assert_eq!(packages.len(), PEER_HEAVY_DEPTH * PEER_HEAVY_WIDTH + 1);
    let (name, manifest) = packages
        .into_iter()
        .find(|(name, _)| name == PEER_HEAVY_PROVIDER)
        .expect("the provider");
    let integrity = manifest["dist"]["integrity"].clone();
    let document = publish_document(&name, manifest);
    let attachment = &document["_attachments"]["peer-benchmark-provider-1.0.0.tgz"];
    let archive = BASE64
        .decode(attachment["data"].as_str().expect("attached data"))
        .unwrap();
    assert_eq!(attachment["length"], archive.len());
    let integrity: ssri::Integrity = integrity
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    integrity.check(&archive).expect("the attached archive matches the version's dist");
    assert_eq!(document["versions"]["1.0.0"]["dist"]["integrity"], integrity.to_string());
}
