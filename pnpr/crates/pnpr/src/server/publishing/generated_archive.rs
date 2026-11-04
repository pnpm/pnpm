//! The `any` archive pnpr generates for a version published without one
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#model)).
//!
//! npm's documents describe a version by one archive. For a version whose
//! publisher sent only variants, pnpr generates a minimal one: a
//! `package.json` with the version's name and version and the union of its
//! variants' `os`, `cpu`, and Linux `libc`, so an npm client reports an
//! unsupported platform instead of installing an empty package. It is built
//! deterministically from those fields, so every replica and every rebuild
//! produces the same bytes. It is not one of the version's archives: pnpr's
//! document and the release record leave it out, and provenance does not
//! name it.

use super::{
    attachments::{AttachmentKind, PreparedAttachment},
    variants::STORED_FIELD,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, GzBuilder};
use pnpm_platform_variants::{Libc, Platform};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::publish::{AttachmentPayload, PendingAttachment};
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, io::Write as _};

/// The attachment that writes the generated archive of `version`, whose
/// stored variants `manifest` lists, through the publish's own write path.
pub(super) fn attachment(
    name: &CanonicalPackageName,
    version: &str,
    manifest: &Value,
) -> PreparedAttachment {
    let tgz = archive(name.as_str(), version, &platforms(manifest));
    let canonical = name.tarball_name_for_version(version);
    let dist = json!({ "integrity": integrity(&tgz) });
    PreparedAttachment {
        attachment: PendingAttachment {
            filename: canonical.clone(),
            declared_length: Some(tgz.len() as u64),
            payload: AttachmentPayload::Inline(BASE64.encode(&tgz)),
            declared_integrity: None,
        },
        canonical,
        version: version.to_string(),
        dist,
        kind: AttachmentKind::Tarball,
        variant_tag: None,
    }
}

/// The platforms the stored variants of a version list.
fn platforms(manifest: &Value) -> Vec<Platform> {
    manifest
        .get(STORED_FIELD)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|variant| variant.get("platforms"))
        .filter_map(|platforms| serde_json::from_value::<Vec<Platform>>(platforms.clone()).ok())
        .flatten()
        .collect()
}

/// The generated `package.json`: `os`, `cpu`, and `libc` each list every
/// value a platform has, sorted. A `wasm32` variant runs on every host, so
/// with one the fields are left out. `libc` is listed only when a Linux
/// platform is, a Linux platform without one counting as glibc.
fn package_json(name: &str, version: &str, platforms: &[Platform]) -> Value {
    let mut manifest = Map::new();
    manifest.insert("name".into(), Value::from(name));
    manifest.insert("version".into(), Value::from(version));
    if platforms.iter().any(|platform| platform.wasm32) {
        return Value::Object(manifest);
    }
    let os: BTreeSet<&str> = platforms
        .iter()
        .filter_map(|platform| platform.os.as_deref())
        .collect();
    let cpu: BTreeSet<&str> = platforms
        .iter()
        .filter_map(|platform| platform.cpu.as_deref())
        .collect();
    let libc: BTreeSet<&str> = platforms
        .iter()
        .filter(|platform| platform.os.as_deref() == Some("linux"))
        .map(|platform| match platform.libc {
            Some(Libc::Musl) => "musl",
            Some(Libc::Glibc) | None => "glibc",
        })
        .collect();
    for (field, values) in [("os", os), ("cpu", cpu), ("libc", libc)] {
        if !values.is_empty() {
            manifest.insert(field.into(), json!(values));
        }
    }
    Value::Object(manifest)
}

/// The generated `.tgz`: one `package/package.json` entry, with every header
/// field and the gzip header fixed.
fn archive(name: &str, version: &str, platforms: &[Platform]) -> Vec<u8> {
    let manifest = serde_json::to_vec_pretty(&package_json(name, version, platforms))
        .expect("a manifest serializes");
    let mut header = tar::Header::new_ustar();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header.set_entry_type(tar::EntryType::Regular);
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_data(&mut header, "package/package.json", manifest.as_slice())
        .expect("writing to memory does not fail");
    let tar = builder.into_inner().expect("writing to memory does not fail");
    let mut gzip = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::best());
    gzip.write_all(&tar).expect("writing to memory does not fail");
    gzip.finish().expect("writing to memory does not fail")
}

fn integrity(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

#[cfg(test)]
mod tests;
