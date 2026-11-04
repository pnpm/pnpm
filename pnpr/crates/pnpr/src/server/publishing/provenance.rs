//! The publisher's provenance statement of a version names every archive the
//! version publishes
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#publication)),
//! so no archive enters the version outside it.
//!
//! The statement is the in-toto payload of the `.sigstore` bundle's DSSE
//! envelope. Each archive is a subject named by its PURL, a variant's with
//! the `variant` qualifier, with the SHA-512 of its `.tgz` as npm provenance
//! names it. The signature is not checked here.

use super::{
    attachments::{AttachmentKind, PreparedAttachment, PublishedTar},
    release_records::{variant_purl, version_purl},
    variants::PublishedVariant,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_release_record::{IN_TOTO_PAYLOAD_TYPE, Purl, STATEMENT_TYPE, Statement};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use serde_json::Value;
use ssri::{Algorithm, Integrity};
use std::{collections::BTreeMap, fmt::Write as _};

/// The in-toto statement types a provenance bundle may carry: npm's older
/// provenance uses v0.1.
const STATEMENT_TYPES: [&str; 2] = [STATEMENT_TYPE, "https://in-toto.io/Statement/v0.1"];

/// The subjects of each version's provenance statement, or why its bundle
/// holds none, by version.
pub(super) type Statements = BTreeMap<String, Result<Vec<(String, String)>, String>>;

/// Read the statement of every `.sigstore` attachment of a publish, before
/// the attachments are written.
pub(super) fn read_statements(prepared: &[PreparedAttachment]) -> Statements {
    prepared
        .iter()
        .filter(|attachment| attachment.kind == AttachmentKind::Sigstore)
        .map(|attachment| {
            let data = attachment.attachment.inline_data().unwrap_or_default();
            (attachment.version.clone(), statement_subjects(data))
        })
        .collect()
}

/// Each subject's name and lowercase hex SHA-512, from a bundle carried as
/// JSON or as base64-encoded JSON.
fn statement_subjects(data: &str) -> Result<Vec<(String, String)>, String> {
    let bundle: Value = serde_json::from_str(data)
        .or_else(|_| {
            let decoded = BASE64.decode(data.trim()).map_err(|err| err.to_string())?;
            serde_json::from_slice(&decoded).map_err(|err| err.to_string())
        })
        .map_err(|err| format!("its bundle is not JSON: {err}"))?;
    let payload = bundle
        .pointer("/dsseEnvelope/payload")
        .and_then(Value::as_str)
        .ok_or("its bundle has no DSSE envelope")?;
    let payload_type = bundle.pointer("/dsseEnvelope/payloadType").and_then(Value::as_str);
    if payload_type != Some(IN_TOTO_PAYLOAD_TYPE) {
        return Err(format!("its DSSE payload type is not {IN_TOTO_PAYLOAD_TYPE}"));
    }
    let payload = BASE64
        .decode(payload.trim())
        .map_err(|err| format!("its DSSE payload is not base64: {err}"))?;
    let statement: Statement = serde_json::from_slice(&payload)
        .map_err(|err| format!("its DSSE payload is not an in-toto statement: {err}"))?;
    if !STATEMENT_TYPES.contains(&statement.statement_type.as_str()) {
        return Err(format!("its statement type {} is not in-toto's", statement.statement_type));
    }
    Ok(statement.subject
        .into_iter()
        .filter_map(|subject| Some((subject.name, subject.digest.get("sha512")?.to_lowercase())))
        .collect())
}

/// Refuse a version with a statement unless the statement names each
/// archive the version publishes: its primary and every variant.
pub(super) fn check_statements(
    name: &CanonicalPackageName,
    statements: &Statements,
    tars: &[PublishedTar],
    variants: &[PublishedVariant],
) -> Result<(), RegistryError> {
    for (version, subjects) in statements {
        let refuse = |reason: String| RegistryError::BadRequest {
            reason: format!(
                "the provenance statement of {}@{version} {reason}; a statement must name every \
                 archive the version publishes",
                name.as_str(),
            ),
        };
        let subjects = subjects.as_ref().map_err(|reason| refuse(reason.clone()))?;
        for (purl, tgz_integrity) in archives(name, version, tars, variants)? {
            let digest = sha512_hex(tgz_integrity.as_deref())
                .ok_or_else(|| {
                    refuse(format!("cannot name {purl}, which has no single SHA-512"))
                })?;
            let named = subjects
                .iter()
                .any(|(subject, sha512)| subject == purl.as_str() && *sha512 == digest);
            if !named {
                return Err(refuse(format!("does not name {purl} with sha512 {digest}")));
            }
        }
    }
    Ok(())
}

/// The PURL and `.tgz` integrity of each archive `version` publishes.
fn archives(
    name: &CanonicalPackageName,
    version: &str,
    tars: &[PublishedTar],
    variants: &[PublishedVariant],
) -> Result<Vec<(Purl, Option<String>)>, RegistryError> {
    let primary = tars
        .iter()
        .filter(|tar| tar.version == version && tar.variant.is_none())
        .map(|tar| Ok((version_purl(name, version)?, tar.tgz_integrity.clone())));
    let variants = variants
        .iter()
        .filter(|variant| variant.version == version)
        .map(|variant| {
            Ok((variant_purl(name, version, &variant.tar.tag)?, variant.tar.tgz_integrity.clone()))
        });
    primary.chain(variants).collect()
}

/// The lowercase hex SHA-512 an SRI `integrity` carries, or `None` unless it
/// carries exactly one. The archive's bytes are verified against any one
/// SHA-512 of its integrity, so only a single one is known to be theirs.
fn sha512_hex(integrity: Option<&str>) -> Option<String> {
    let integrity: Integrity = integrity?.parse().ok()?;
    let mut sha512 = integrity.hashes
        .iter()
        .filter(|hash| hash.algorithm == Algorithm::Sha512);
    let (Some(hash), None) = (sha512.next(), sha512.next()) else {
        return None;
    };
    let bytes = BASE64.decode(&hash.digest).ok()?;
    Some(
        bytes
            .iter()
            .fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            }),
    )
}

#[cfg(test)]
mod tests;
