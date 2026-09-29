use super::{MissingIntegrityTarball, apply_pins};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};
use ssri::{Algorithm, Integrity, IntegrityOpts};

fn sha512(bytes: &[u8]) -> Integrity {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(bytes);
    opts.result()
}

fn pin(version: &str, bytes: &[u8]) -> (MissingIntegrityTarball, Integrity) {
    let candidate = MissingIntegrityTarball {
        version: version.to_string(),
        filename: format!("foo-{version}.tgz"),
        needs_integrity: true,
        expected_shasum: None,
    };
    (candidate, sha512(bytes))
}

fn version(tarball: &str, integrity: Option<&str>) -> Value {
    let mut dist = json!({ "tarball": format!("https://registry.test/foo/-/{tarball}") });
    if let Some(integrity) = integrity {
        dist["integrity"] = json!(integrity);
    }
    json!({ "name": "foo", "dist": dist })
}

/// Pins are computed against the packument pnpr fetched, but land on whatever
/// the cache holds by the time the downloads finish, so a refresh in between
/// keeps what it published.
#[test]
fn pins_apply_only_where_the_current_packument_still_lacks_them() {
    let name = CanonicalPackageName::parse("foo", Ecosystem::Npm).unwrap();
    let republished = sha512(b"republished").to_string();
    let mut doc = json!({
        "name": "foo",
        "versions": {
            "1.0.0": version("foo-1.0.0.tgz", None),
            "2.0.0": version("foo-2.0.0.tgz", Some(&republished)),
            "3.0.0": version("foo-3.0.0-renamed.tgz", None),
        },
    });

    let applied = apply_pins(
        &mut doc,
        &name,
        vec![
            pin("1.0.0", b"one"),
            pin("2.0.0", b"two"),
            pin("3.0.0", b"three"),
            pin("4.0.0", b"four"),
        ],
    );

    assert!(applied);
    let versions = &doc["versions"];
    assert_eq!(versions["1.0.0"]["dist"]["integrity"], sha512(b"one").to_string());
    assert_eq!(versions["2.0.0"]["dist"]["integrity"], republished);
    assert!(versions["3.0.0"]["dist"].get("integrity").is_none());
    assert!(versions.get("4.0.0").is_none());
}

#[test]
fn nothing_is_applied_when_every_pin_is_stale() {
    let name = CanonicalPackageName::parse("foo", Ecosystem::Npm).unwrap();
    let mut doc = json!({ "name": "foo", "versions": {} });

    assert!(!apply_pins(&mut doc, &name, vec![pin("1.0.0", b"one")]));
}
