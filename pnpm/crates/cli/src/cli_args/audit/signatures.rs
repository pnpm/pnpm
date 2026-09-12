//! `pacquet audit signatures` — verify ECDSA registry signatures for the
//! installed packages.
//!
//! For every installed `name@version`, the package's own registry is asked
//! for its signing keys (`/-/npm/v1/keys`) and its full packument. A
//! package is **verified** as soon as one of its `dist.signatures` validates,
//! over the message `name@version:integrity`, against a trusted
//! ECDSA-P256 key. Registries that advertise no signing keys are skipped
//! (there is no trust root to check against); a package whose registry does
//! provide keys but whose signature is absent is **missing**, and one whose
//! signature is present but does not validate is **invalid** — a tamper
//! signal.

use super::{bold, red, retry_opts_from_config, sanitize_response_body};
use base64::Engine as _;
use owo_colors::{OwoColorize, Stream};
use p256::{
    ecdsa::{Signature, VerifyingKey, signature::Verifier},
    pkcs8::DecodePublicKey,
};
use pnpm_config::Config;
use pnpm_network::{ThrottledClient, encode_package_name, redact_url_credentials, send_with_retry};
use registry::{
    PackageSignature, Packument, RegistryKey, fetch_packument, fetch_registry_keys, parse_timestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// One installed package to check, already routed to the registry it was
/// installed from.
pub(super) struct SignaturePackage {
    pub name: String,
    pub registry: String,
    pub version: String,
}

/// A package that failed (or lacks) signature verification. JSON-serialized
/// in the `--json` report; `integrity`, `reason`, and `resolved` are omitted
/// when absent, matching pnpm's `JSON.stringify` dropping `undefined`.
#[derive(Debug, Serialize)]
pub(super) struct SignatureIssue {
    pub name: String,
    pub registry: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct SignatureVerificationResult {
    pub audited: usize,
    pub invalid: Vec<SignatureIssue>,
    pub missing: Vec<SignatureIssue>,
    pub verified: usize,
}

#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[non_exhaustive]
pub(super) enum SignaturesError {
    // `reason` is the registry error already passed through
    // `redact_url_credentials`; the raw `reqwest::Error` is not carried as a
    // diagnostic source, so embedded `user:pass@` credentials cannot leak via
    // its `Display` or the miette cause chain.
    #[display("Failed to request the registry keys endpoint (at {url}): {reason}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysNetwork { url: String, reason: String },

    #[display("The registry keys endpoint (at {url}) responded with {status}: {body}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysBadStatus { url: String, status: u16, body: String },

    #[display(
        "The registry keys endpoint (at {url}) returned invalid JSON: {reason}. Response body: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysInvalidJson { url: String, reason: String, body: String },

    #[display(
        "The registry keys endpoint (at {url}) returned an unexpected body. Expected an object with a keys array; got: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysUnexpectedBody { url: String, body: String },

    /// See [`SignaturesError::KeysNetwork`] for why the error is stored as a
    /// pre-redacted string rather than a `reqwest::Error` source.
    #[display("Failed to request the packument endpoint (at {url}): {reason}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentNetwork { url: String, reason: String },

    #[display("The packument endpoint (at {url}) responded with {status}: {body}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentBadStatus { url: String, status: u16, body: String },

    #[display(
        "The packument endpoint (at {url}) returned invalid JSON: {reason}. Response body: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentInvalidJson { url: String, reason: String, body: String },

    #[display(
        "The packument endpoint (at {url}) returned an unexpected body. Expected an object with versions; got: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentUnexpectedBody { url: String, body: String },
}

/// Verify registry signatures for every package in `packages`. Keys are
/// fetched once per registry; packuments once per `(registry, name)` and
/// reused across a package's installed versions. A keys-endpoint failure is
/// fatal (no trust root); a packument failure is recorded against just the
/// packages that needed it, mirroring pnpm's per-package `catch`.
pub(super) async fn verify_signatures(
    packages: &[SignaturePackage],
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<SignatureVerificationResult, SignaturesError> {
    let keys_by_registry = fetch_keys_by_registry(packages, config, http_client).await?;
    let packuments =
        fetch_needed_packuments(packages, &keys_by_registry, config, http_client).await;

    let mut result = SignatureVerificationResult::default();
    for pkg in packages {
        let Some(keys) = keys_by_registry.get(&pkg.registry).filter(|keys| !keys.is_empty()) else {
            continue;
        };
        match packuments.get(&(pkg.registry.clone(), pkg.name.clone())) {
            Some(Err(reason)) => {
                result.invalid.push(issue(pkg, None, None, Some(reason.clone())));
            }
            Some(Ok(None)) | None => {}
            Some(Ok(Some(packument))) => {
                result.audited += 1;
                process_version(pkg, packument, keys, &mut result);
            }
        }
    }

    result.invalid.sort_by_key(sort_key);
    result.missing.sort_by_key(sort_key);
    Ok(result)
}

async fn fetch_keys_by_registry(
    packages: &[SignaturePackage],
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<HashMap<String, Vec<RegistryKey>>, SignaturesError> {
    let registries: BTreeSet<&str> = packages.iter().map(|pkg| pkg.registry.as_str()).collect();
    let key_fetches = registries.into_iter().map(|registry| async move {
        fetch_registry_keys(registry, config, http_client)
            .await
            .map(|keys| (registry.to_string(), keys))
    });
    Ok(futures_util::future::try_join_all(key_fetches).await?.into_iter().collect())
}

/// The packuments of the packages whose registry advertises signing keys;
/// a registry without keys is skipped entirely.
async fn fetch_needed_packuments(
    packages: &[SignaturePackage],
    keys_by_registry: &HashMap<String, Vec<RegistryKey>>,
    config: &Config,
    http_client: &ThrottledClient,
) -> HashMap<(String, String), Result<Option<Packument>, String>> {
    let needed: BTreeSet<(&str, &str)> = packages
        .iter()
        .filter(|pkg| keys_by_registry.get(&pkg.registry).is_some_and(|keys| !keys.is_empty()))
        .map(|pkg| (pkg.registry.as_str(), pkg.name.as_str()))
        .collect();
    let packument_fetches = needed.into_iter().map(|(registry, name)| async move {
        let result = fetch_packument(name, registry, config, http_client)
            .await
            .map_err(|err| err.to_string());
        ((registry.to_string(), name.to_string()), result)
    });
    futures_util::future::join_all(packument_fetches).await.into_iter().collect()
}

fn process_version(
    pkg: &SignaturePackage,
    packument: &Packument,
    keys: &[RegistryKey],
    result: &mut SignatureVerificationResult,
) {
    let version = packument.versions.get(&pkg.version);
    let published_at =
        packument.time.get(&pkg.version).and_then(serde_json::Value::as_str).map(str::to_string);
    let dist = version.and_then(|version| version.dist.as_ref());
    let integrity = dist.and_then(|dist| dist.integrity.clone());
    let resolved = dist.and_then(|dist| dist.tarball.clone());
    let raw_signatures = dist.and_then(|dist| dist.signatures.as_ref());

    let Some(signatures) = parse_signatures(raw_signatures) else {
        result.invalid.push(issue(pkg, integrity, resolved, Some(malformed_reason(pkg))));
        return;
    };

    if version.is_none() {
        let reason = format!("Missing registry metadata for {}@{}", pkg.name, pkg.version);
        result.invalid.push(issue(pkg, None, None, Some(reason)));
        return;
    }
    let Some(integrity) = integrity else {
        result.missing.push(issue(pkg, None, resolved, None));
        return;
    };
    if signatures.is_empty() {
        result.missing.push(issue(pkg, Some(integrity), resolved, None));
        return;
    }

    match verify_package_signatures(
        pkg,
        &integrity,
        published_at.as_deref(),
        resolved.as_deref(),
        &signatures,
        keys,
    ) {
        Some(invalid) => result.invalid.push(invalid),
        None => result.verified += 1,
    }
}

/// The `dist.signatures` entries; `None` when the field is present but is
/// not an array of well-formed signatures.
fn parse_signatures(raw_signatures: Option<&serde_json::Value>) -> Option<Vec<PackageSignature>> {
    let Some(value) = raw_signatures else {
        return Some(Vec::new());
    };
    let serde_json::Value::Array(elements) = value else {
        return None;
    };
    elements.iter().map(|element| serde_json::from_value(element.clone()).ok()).collect()
}

/// Returns `None` as soon as one signature validates against a trusted key.
/// Unknown-key, expired-key, and invalid-signature outcomes are recorded but
/// do not on their own fail the package — only the absence of any valid
/// signature does. This keeps a key rotation (multiple signatures in the
/// packument) working and stops a mirror from forcing a failure by appending
/// junk. The surfaced reason prefers an invalid-signature failure (a tamper
/// signal) over the weaker unknown/expired reasons.
fn verify_package_signatures(
    pkg: &SignaturePackage,
    integrity: &str,
    published_at: Option<&str>,
    resolved: Option<&str>,
    signatures: &[PackageSignature],
    keys: &[RegistryKey],
) -> Option<SignatureIssue> {
    let message = format!("{}@{}:{integrity}", pkg.name, pkg.version);
    let published_time = published_at.and_then(parse_timestamp);

    let mut failures = Vec::new();
    for signature in signatures {
        let Some(key) = keys.iter().find(|key| key.keyid == signature.keyid) else {
            failures.push(format!(
                "{}@{} has a registry signature with keyid {} but no corresponding public key can be found",
                pkg.name, pkg.version, signature.keyid,
            ));
            continue;
        };
        // Key expiry is a consistency check, not a security boundary: the
        // publish time comes from the same unauthenticated packument as the
        // signatures. A missing or unparsable publish time therefore keeps the
        // key usable — the signature check below is what gates acceptance.
        let expired = match (key.expires.as_deref().and_then(parse_timestamp), published_time) {
            (Some(expires), Some(published)) => published >= expires,
            _ => false,
        };
        if expired {
            failures.push(format!(
                "{}@{} has a registry signature with keyid {} but the corresponding public key has expired {}",
                pkg.name,
                pkg.version,
                signature.keyid,
                key.expires.as_deref().unwrap_or_default(),
            ));
            continue;
        }
        if verify_one(&key.key, &message, &signature.sig) {
            return None;
        }
        failures.push(format!(
            "{}@{} has an invalid registry signature with keyid {}",
            pkg.name, pkg.version, signature.keyid,
        ));
    }

    Some(issue(
        pkg,
        Some(integrity.to_string()),
        resolved.map(str::to_string),
        Some(most_telling_failure(pkg, &failures)),
    ))
}

/// Verify one base64 ECDSA-P256 signature over `message` against a base64
/// SPKI public key. Any malformed key material or signature bytes count as a
/// non-match rather than an error, so one bad key can't abort the audit.
fn verify_one(public_key_base64: &str, message: &str, signature_base64: &str) -> bool {
    let engine = base64::engine::general_purpose::STANDARD;
    let Ok(key_der) = engine.decode(public_key_base64) else { return false };
    let Ok(verifying_key) = VerifyingKey::from_public_key_der(&key_der) else { return false };
    let Ok(signature_der) = engine.decode(signature_base64) else { return false };
    let Ok(signature) = Signature::from_der(&signature_der) else { return false };
    verifying_key.verify(message.as_bytes(), &signature).is_ok()
}

fn most_telling_failure(pkg: &SignaturePackage, failures: &[String]) -> String {
    if failures.is_empty() {
        return format!(
            "{}@{} has no registry signature from a trusted key",
            pkg.name, pkg.version,
        );
    }
    failures
        .iter()
        .find(|reason| reason.contains("invalid registry signature"))
        .cloned()
        .unwrap_or_else(|| failures[0].clone())
}

fn issue(
    pkg: &SignaturePackage,
    integrity: Option<String>,
    resolved: Option<String>,
    reason: Option<String>,
) -> SignatureIssue {
    SignatureIssue {
        name: pkg.name.clone(),
        registry: pkg.registry.clone(),
        version: pkg.version.clone(),
        integrity,
        reason,
        resolved,
    }
}

fn malformed_reason(pkg: &SignaturePackage) -> String {
    format!("Malformed registry signatures metadata for {}@{}", pkg.name, pkg.version)
}

fn sort_key(issue: &SignatureIssue) -> String {
    format!("{}@{}", issue.name, issue.version)
}

pub(super) fn render_signature_verification_result(result: &SignatureVerificationResult) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("audited {} {}", result.audited, plural(result.audited, "package")));
    lines.push(String::new());

    if result.verified > 0 {
        lines.push(format!(
            "{} {} {} registry {}",
            result.verified,
            if result.verified == 1 { "package has a" } else { "packages have" },
            bold("verified"),
            plural(result.verified, "signature"),
        ));
        lines.push(String::new());
    }
    push_missing_signatures(&mut lines, &result.missing);
    push_invalid_signatures(&mut lines, &result.invalid);

    if result.audited == 0
        && result.invalid.is_empty()
        && result.missing.is_empty()
        && result.verified == 0
    {
        lines.push("No dependencies were installed from a registry with signing keys".to_string());
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Packages the registry has signing keys for but published unsigned.
fn push_missing_signatures(lines: &mut Vec<String>, missing: &[SignatureIssue]) {
    let count = missing.len();
    if count == 0 {
        return;
    }
    lines.push(format!(
        "{count} {} {} registry {} but the registry is providing signing keys:",
        if count == 1 { "package is" } else { "packages are" },
        bright_red("missing"),
        plural(count, "signature"),
    ));
    lines.push(String::new());
    lines.push(issue_table(missing, false));
    lines.push(String::new());
}

/// Packages whose signature did not verify — the tampering warning.
fn push_invalid_signatures(lines: &mut Vec<String>, invalid: &[SignatureIssue]) {
    let count = invalid.len();
    if count == 0 {
        return;
    }
    lines.push(format!(
        "{count} {} {} registry {}:",
        if count == 1 { "package has an" } else { "packages have" },
        bright_red("invalid"),
        plural(count, "signature"),
    ));
    lines.push(String::new());
    lines.push(issue_table(invalid, true));
    lines.push(String::new());
    lines.push(
        if count == 1 {
            "Someone might have tampered with this package since it was published on the registry!"
        } else {
            "Someone might have tampered with these packages since they were published on the registry!"
        }
        .to_string(),
    );
    lines.push(String::new());
}

fn issue_table(issues: &[SignatureIssue], with_reason: bool) -> String {
    use tabled::{builder::Builder, settings::Style};

    let mut builder = Builder::default();
    for issue in issues {
        let package = red(&format!("{}@{}", issue.name, issue.version));
        if with_reason {
            let reason =
                issue.reason.clone().unwrap_or_else(|| "Invalid registry signature".to_string());
            builder.push_record(vec![package, issue.registry.clone(), reason]);
        } else {
            builder.push_record(vec![package, issue.registry.clone()]);
        }
    }
    let mut table = builder.build();
    table.with(Style::modern());
    table.to_string()
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 { word.to_string() } else { format!("{word}s") }
}

fn bright_red(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bright_red()).to_string()
}

#[cfg(test)]
mod tests;

mod registry;
