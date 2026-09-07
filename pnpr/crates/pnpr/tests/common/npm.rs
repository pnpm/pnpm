//! The npm publish fixtures the integration suites send: the document a
//! client `PUT`s for one version, and the two digests it carries.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use std::fmt::Write;

/// The publish document an npm client sends for one version of `name`, with
/// `tarball` attached. Scoped names take their filename from the last
/// segment, as npm does.
pub fn publish_doc(name: &str, version: &str, tarball: &[u8]) -> Value {
    let basename = name.rsplit('/').next().unwrap_or(name);
    let filename = format!("{basename}-{version}.tgz");
    json!({
        "_id": name,
        "name": name,
        "description": "test",
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": name,
                "version": version,
                "dist": {
                    "tarball": format!("http://localhost:4873/{name}/-/{filename}"),
                    "shasum": sha1_hex(tarball),
                    "integrity": sri_sha512(tarball),
                }
            }
        },
        "_attachments": {
            filename: {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(tarball),
                "length": tarball.len()
            }
        }
    })
}

/// The SRI `sha512-...` string npm clients send in `dist.integrity`.
pub fn sri_sha512(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

/// The 40-char hex SHA-1 npm clients send in the legacy `dist.shasum` field.
pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha1);
    opts.input(bytes);
    let integrity = opts.result();
    let digest_bytes = BASE64.decode(&integrity.hashes[0].digest).unwrap();
    digest_bytes.iter().fold(String::with_capacity(40), |mut hex, byte| {
        write!(hex, "{byte:02x}").unwrap();
        hex
    })
}
