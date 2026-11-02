//! The npm publish fixtures the integration suites send: the document a
//! client `PUT`s for one version, and the two digests it carries.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use std::{fmt::Write, io::Write as _};

/// A `.tgz` an npm client would publish for `name@version`: a
/// `package/package.json` naming the version, and `payload` as
/// `package/payload`, so two payloads make two archives. Deterministic.
pub fn npm_tgz(name: &str, version: &str, payload: &[u8]) -> Vec<u8> {
    npm_tgz_with(&json!({ "name": name, "version": version }), payload)
}

/// A `.tgz` whose `package/package.json` is `manifest`, with `payload` as
/// `package/payload`.
pub fn npm_tgz_with(manifest: &Value, payload: &[u8]) -> Vec<u8> {
    let manifest = manifest.to_string();
    tgz_of(&[("package/package.json", manifest.as_bytes()), ("package/payload", payload)])
}

/// A deterministic `.tgz` holding `entries` as regular files.
pub fn tgz_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        builder.append_data(&mut header, path, *data).unwrap();
    }
    let tar = builder.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).unwrap();
    encoder.finish().unwrap()
}

/// The `package.json` an npm client reads out of `tgz` and submits as the
/// version's metadata, or `None` when it holds none.
pub fn archive_manifest(tgz: &[u8]) -> Option<Value> {
    use std::io::Read as _;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tgz));
    for entry in archive.entries().ok()? {
        let mut entry = entry.ok()?;
        let path = entry
            .path()
            .ok()?
            .to_string_lossy()
            .into_owned();
        if path
            .split('/')
            .skip(1)
            .collect::<Vec<_>>()
            == ["package.json"]
        {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).ok()?;
            return serde_json::from_slice(&bytes).ok();
        }
    }
    None
}

/// The version metadata a client submits for `tgz`: its `package.json`, with
/// `dist`.
pub fn version_metadata(tgz: &[u8], name: &str, version: &str, dist: Value) -> Value {
    let mut metadata = archive_manifest(tgz).unwrap_or_else(|| json!({}));
    metadata["name"] = json!(name);
    metadata["version"] = json!(version);
    metadata["dist"] = dist;
    metadata
}

/// `tarball` when it already is a gzip stream, otherwise [`npm_tgz`] around it:
/// pnpr verifies every published archive's `package.json` against the
/// submitted metadata, so a fixture's bytes must be a real package archive.
pub fn publishable_tgz(name: &str, version: &str, tarball: &[u8]) -> Vec<u8> {
    if tarball.starts_with(&[0x1f, 0x8b]) {
        tarball.to_vec()
    } else {
        npm_tgz(name, version, tarball)
    }
}

/// The publish document an npm client sends for one version of `name`, with
/// `tarball` attached. Scoped names take their filename from the last
/// segment, as npm does.
pub fn publish_doc(name: &str, version: &str, tarball: &[u8]) -> Value {
    let tarball = &publishable_tgz(name, version, tarball);
    let basename = name.rsplit('/').next().unwrap_or(name);
    let filename = format!("{basename}-{version}.tgz");
    json!({
        "_id": name,
        "name": name,
        "description": "test",
        "dist-tags": { "latest": version },
        "versions": {
            version: version_metadata(tarball, name, version, json!({
                "tarball": format!("http://localhost:4873/{name}/-/{filename}"),
                "shasum": sha1_hex(tarball),
                "integrity": sri_sha512(tarball),
            })),
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
    let digest_bytes = BASE64
        .decode(&integrity.hashes[0].digest)
        .unwrap();
    digest_bytes
        .iter()
        .fold(String::with_capacity(40), |mut hex, byte| {
            write!(hex, "{byte:02x}").unwrap();
            hex
        })
}
