use super::{MissingIntegrityTarball, pin_still_applies, set_integrity};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};
use ssri::{Algorithm, Integrity, IntegrityOpts};

fn digest(algorithm: Algorithm, bytes: &[u8]) -> Integrity {
    let mut opts = IntegrityOpts::new().algorithm(algorithm);
    opts.input(bytes);
    opts.result()
}

fn pinned(version: &str, shasum_of: Option<&[u8]>) -> MissingIntegrityTarball {
    MissingIntegrityTarball {
        version: version.to_string(),
        filename: format!("foo-{version}.tgz"),
        needs_integrity: true,
        expected_shasum: shasum_of.map(|bytes| digest(Algorithm::Sha1, bytes)),
    }
}

fn version(tarball: &str, shasum_of: Option<&[u8]>, integrity: Option<&str>) -> Value {
    let mut dist = json!({ "tarball": format!("https://registry.test/foo/-/{tarball}") });
    if let Some(bytes) = shasum_of {
        dist["shasum"] = json!(digest(Algorithm::Sha1, bytes).to_hex().1);
    }
    if let Some(integrity) = integrity {
        dist["integrity"] = json!(integrity);
    }
    json!({ "name": "foo", "dist": dist })
}

/// Pins are computed against the packument pnpr fetched, but land on whatever
/// the cache holds by the time the downloads finish, so a refresh in between
/// keeps what it published.
#[test]
fn a_pin_applies_only_where_the_current_packument_still_lacks_it() {
    let name = CanonicalPackageName::parse("foo", Ecosystem::Npm).unwrap();
    let republished = digest(Algorithm::Sha512, b"republished").to_string();
    let doc = json!({
        "name": "foo",
        "versions": {
            "1.0.0": version("foo-1.0.0.tgz", Some(b"one"), None),
            "2.0.0": version("foo-2.0.0.tgz", None, Some(&republished)),
            "3.0.0": version("foo-3.0.0-renamed.tgz", None, None),
            "5.0.0": version("foo-5.0.0.tgz", Some(b"five, republished"), None),
        },
    });

    assert!(pin_still_applies(&doc, &name, &pinned("1.0.0", Some(b"one"))));
    assert!(!pin_still_applies(&doc, &name, &pinned("2.0.0", None)), "gained an integrity");
    assert!(!pin_still_applies(&doc, &name, &pinned("3.0.0", None)), "tarball renamed");
    assert!(!pin_still_applies(&doc, &name, &pinned("4.0.0", None)), "version removed");
    assert!(!pin_still_applies(&doc, &name, &pinned("5.0.0", Some(b"five"))), "shasum changed");
}

#[test]
fn set_integrity_writes_the_pin_into_the_version_dist() {
    let mut doc = json!({
        "name": "foo",
        "versions": { "1.0.0": version("foo-1.0.0.tgz", None, None) },
    });
    let integrity = digest(Algorithm::Sha512, b"one");

    set_integrity(&mut doc, "1.0.0", &integrity);

    assert_eq!(doc["versions"]["1.0.0"]["dist"]["integrity"], integrity.to_string());
}
