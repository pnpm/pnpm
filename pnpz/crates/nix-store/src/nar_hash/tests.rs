use super::NarHash;
use pretty_assertions::assert_eq;

const BASE32: &str = "sha256:0mdqa9w1p6cmli6976v4wi0sw9r4p5prkj7lzfd1877wk11c9c73";
const HEX: &str = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const SRI: &str = "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";

#[test]
fn every_notation_parses_to_the_same_hash() {
    let hash = NarHash::parse(BASE32).unwrap();
    assert_eq!(NarHash::parse(HEX).unwrap(), hash);
    assert_eq!(NarHash::parse(SRI).unwrap(), hash);
}

#[test]
fn prints_the_narinfo_and_lockfile_notations() {
    let hash = NarHash::parse(HEX).unwrap();
    assert_eq!(hash.to_nix_base32(), BASE32);
    assert_eq!(hash.to_sri(), SRI);
}

#[test]
fn rejects_other_algorithms_and_truncated_digests() {
    NarHash::parse("sha1:0mdqa9w1p6cmli6976v4wi0sw9r4p5pr").unwrap_err();
    NarHash::parse("sha256-47DEQpj8HBSa").unwrap_err();
    NarHash::parse("sha256:zz").unwrap_err();
}
