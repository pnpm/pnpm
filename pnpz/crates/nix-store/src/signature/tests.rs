use super::{
    NIXOS_CACHE_KEY,
    TrustedKey,
};
use crate::NarInfo;

const PYTHON: &str = include_str!("../fixtures/python3.narinfo");

fn nixos_key() -> Vec<TrustedKey> {
    vec![TrustedKey::parse(NIXOS_CACHE_KEY).unwrap()]
}

#[test]
fn accepts_a_document_signed_by_the_nixos_cache() {
    NarInfo::parse(PYTHON)
        .unwrap()
        .verify(&nixos_key())
        .unwrap();
}

#[test]
fn rejects_a_document_whose_signed_fields_changed() {
    let resized = PYTHON.replace("NarSize: 133904224", "NarSize: 133904225");
    NarInfo::parse(&resized)
        .unwrap()
        .verify(&nixos_key())
        .unwrap_err();

    let fewer_references = PYTHON.replace("9j086va2zn7ifgdc3h3jlqbi2c23j1x5-mailcap-2.1.54 ", "");
    NarInfo::parse(&fewer_references)
        .unwrap()
        .verify(&nixos_key())
        .unwrap_err();
}

#[test]
fn accepts_a_recompressed_archive() {
    let recompressed =
        PYTHON.replace("Compression: zstd", "Compression: xz").replace(".nar.zst", ".nar.xz");
    NarInfo::parse(&recompressed)
        .unwrap()
        .verify(&nixos_key())
        .unwrap();
}

#[test]
fn rejects_a_signature_by_an_untrusted_key() {
    let other_key = "other-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=";
    let keys = vec![TrustedKey::parse(other_key).unwrap()];
    let error = NarInfo::parse(PYTHON)
        .unwrap()
        .verify(&keys)
        .unwrap_err();
    assert!(error.to_string().contains("cache.nixos.org-1"), "{error}");
}

#[test]
fn rejects_an_unsigned_document() {
    let unsigned = PYTHON
        .lines()
        .filter(|line| !line.starts_with("Sig"))
        .collect::<Vec<_>>();
    NarInfo::parse(&unsigned.join("\n"))
        .unwrap()
        .verify(&nixos_key())
        .unwrap_err();
}

#[test]
fn rejects_a_malformed_key() {
    TrustedKey::parse("cache.nixos.org-1").unwrap_err();
    TrustedKey::parse("cache.nixos.org-1:AAAA").unwrap_err();
}
