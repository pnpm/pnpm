//! A retried publish to pnpr that signs provenance again sends a new
//! statement, so pnpr does not take it for the request it repeats and
//! answers that the version is already published. pnpm then reads the
//! version's release record, its provenance statement, and its dist-tags,
//! and reports the version as published only when they are the release this
//! publish would have made
//! ([platform variants](https://github.com/pnpm/rfcs/blob/e920906d80bfb372068eab901fca2bb43fb1b97f/pnpr/text/0000-platform-variants.md#dry-runs-provenance-and-retries)).

use super::{
    PackedPkg, PublishNetwork, PublishPackedPkgError, PublishPackedPkgOptions, PublishedPkg,
    clean_version, escaped_package_name, finish_publish, publish_authorization,
    request::PublishResponse, variants,
};
use crate::{
    ResolvedPublishOptions, global_log::global_info, provenance_gen::ProvenanceSubject,
    publish_summary::PublishSummary, registry_config_keys::NormalizedRegistryUrl,
};
use base64::Engine as _;
use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_network::{ThrottledClient, read_limited_body};
use pnpm_reporter::Reporter;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The largest record, statement, or dist-tags document read.
const BODY_LIMIT: usize = 8 * 1024 * 1024;

/// The release a publish would have made of one version.
pub(super) struct Release<'a> {
    pub(super) name: &'a str,
    /// The version without build metadata, as the registry names it.
    pub(super) version: &'a str,
    /// The `.tgz` integrity of the `any` archive, when the version has one.
    pub(super) primary: Option<String>,
    /// Each platform variant's `.tgz` integrity, by its tag.
    pub(super) variants: BTreeMap<String, String>,
    /// The subjects of the provenance statement this publish signs, when it
    /// signs one.
    pub(super) subjects: Option<Vec<ProvenanceSubject>>,
    /// The dist-tag the publish sets.
    pub(super) tag: &'a str,
}

impl<'a> Release<'a> {
    /// The release `pkg` makes of `name@version`, setting `tag`, with the
    /// provenance statement it signs when it `signs`.
    pub(super) fn of(
        pkg: &PackedPkg<'_>,
        (name, version): (&'a str, &'a str),
        tag: &'a str,
        signs: bool,
    ) -> Result<Self, PublishPackedPkgError> {
        let primary = match pkg.any_staged {
            Some(staged) => Some(staged.integrity.clone()),
            None => pkg.has_any_archive().then(|| variants::sri_sha512(pkg.tarball_data)),
        };
        let variants = pkg.variants
            .iter()
            .map(|variant| (variant.tag.clone(), variant.archive.integrity()))
            .collect();
        let subjects = signs
            .then(|| variants::provenance_subjects(pkg, (name, version)))
            .transpose()?;
        Ok(Release { name, version, primary, variants, subjects, tag })
    }
}

/// Record what the registry answered, which may be that the version is
/// already published as the release a retried publish makes.
pub(super) async fn settle_publish<Reporter: self::Reporter>(
    pkg: &PackedPkg<'_>,
    (opts, network, resolved): (
        &PublishPackedPkgOptions,
        &PublishNetwork<'_>,
        &ResolvedPublishOptions,
    ),
    response: PublishResponse,
    (summary, published): (&mut PublishSummary, &PublishedPkg<'_>),
) -> Result<(), PublishPackedPkgError> {
    let PublishedPkg { name, version, .. } = *published;
    if is_retried_release(pkg, (opts, network), resolved, &response, (name, version)).await? {
        global_info::<Reporter>(&format!(
            "✅ {name}@{version} is already published as this release",
        ));
        return Ok(());
    }
    finish_publish::<Reporter>(response, summary, published)
}

/// Whether pnpr refused the publish of `name@version` as already published,
/// and the published version is the release `pkg` makes: then the publish
/// is a retry of one whose answer was lost, and it succeeded. Fails naming
/// what differs when the published version is another release.
async fn is_retried_release(
    pkg: &PackedPkg<'_>,
    (opts, network): (&PublishPackedPkgOptions, &PublishNetwork<'_>),
    resolved: &ResolvedPublishOptions,
    response: &PublishResponse,
    (name, version): (&str, &str),
) -> Result<bool, PublishPackedPkgError> {
    let registry = &resolved.registry;
    if response.status != 409 || opts.stage || !opts.pnpr_registries.contains(registry.as_str()) {
        return Ok(false);
    }
    let version = clean_version(version)?;
    let signs = resolved.provenance == Some(true);
    let release = Release::of(pkg, (name, &version), &resolved.default_tag, signs)?;
    let authorization = publish_authorization(resolved, network, registry, name);
    confirm_published(network.client, registry, authorization.as_deref(), &release)
        .await
        .map_err(PublishPackedPkgError::PublishedRelease)?;
    Ok(true)
}

/// Confirm that the version pnpr reports as already published is `release`:
/// its record names the same archives, it has the provenance statement this
/// publish signs, and its dist-tag points at it. Fails naming what differs.
pub(super) async fn confirm_published(
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
    release: &Release<'_>,
) -> Result<(), PublishedReleaseError> {
    let reader = Reader { client, registry, authorization, release };
    let escaped = escaped_package_name(release.name);
    let record = reader
        .get(&format!("{escaped}/{}+r0", release.version))
        .await?
        .ok_or_else(|| reader.differs("pnpr has no release record of the version".to_string()))?;
    if let Some(difference) = archive_difference(&record, release) {
        return Err(reader.differs(difference));
    }
    if let Some(subjects) = &release.subjects {
        let spec = format!("{escaped}@{}", release.version);
        let attestations = reader.get(&format!("-/npm/v1/attestations/{spec}")).await?;
        if !names_every_archive(attestations.as_ref(), subjects) {
            return Err(reader.differs(
                "the version has no provenance statement naming every archive this publish signs for"
                    .to_string(),
            ));
        }
    }
    let dist_tags = reader.get(&format!("-/package/{escaped}/dist-tags")).await?;
    let tagged = dist_tags
        .as_ref()
        .and_then(|tags| tags.get(release.tag))
        .and_then(Value::as_str);
    if tagged != Some(release.version) {
        let points_at =
            tagged.map_or_else(|| "is not set".to_string(), |at| format!("points at {at}"));
        return Err(reader.differs(format!("the dist-tag `{}` {points_at}", release.tag)));
    }
    Ok(())
}

/// What a recorded release's archives differ by from `release`'s, if
/// anything. A version without an `any` archive is compared by its variants
/// only: its record names no primary input.
fn archive_difference(record: &Value, release: &Release<'_>) -> Option<String> {
    if let Some(primary) = &release.primary {
        let recorded = input_integrity(record);
        if recorded != Some(primary.as_str()) {
            return Some(archive_mismatch("the `any` archive", recorded, primary));
        }
    }
    let recorded: BTreeMap<&str, Option<&str>> = record
        .get("variants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|variant| Some((variant.get("tag")?.as_str()?, input_integrity(variant))))
        .collect();
    let recorded_tags: BTreeSet<&str> = recorded.keys().copied().collect();
    let tags: BTreeSet<&str> = release.variants
        .keys()
        .map(String::as_str)
        .collect();
    if recorded_tags != tags {
        return Some(format!(
            "the version is recorded with the variants [{}], and this publish has [{}]",
            joined(&recorded_tags),
            joined(&tags),
        ));
    }
    release.variants
        .iter()
        .find_map(|(tag, integrity)| {
            let recorded = recorded
                .get(tag.as_str())
                .copied()
                .flatten();
            (recorded != Some(integrity.as_str())).then(|| {
                archive_mismatch(&format!("the variant `{tag}`"), recorded, integrity)
            })
        })
}

/// The `.tgz` integrity a record, or one of its variant entries, names as its
/// input.
fn input_integrity(entry: &Value) -> Option<&str> {
    entry.pointer("/inputs/archive/integrity").and_then(Value::as_str)
}

fn archive_mismatch(archive: &str, recorded: Option<&str>, integrity: &str) -> String {
    let recorded = recorded.map_or_else(|| "no archive".to_string(), |at| format!("`{at}`"));
    format!("{archive} is recorded as {recorded}, and this publish has `{integrity}`")
}

fn joined(tags: &BTreeSet<&str>) -> String {
    tags.iter()
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether one of the `attestations` is a statement whose subjects are
/// exactly `subjects`.
fn names_every_archive(attestations: Option<&Value>, subjects: &[ProvenanceSubject]) -> bool {
    let expected: BTreeSet<(&str, &str)> = subjects
        .iter()
        .map(|subject| (subject.name.as_str(), subject.sha512_hex.as_str()))
        .collect();
    attestations
        .and_then(|response| response.get("attestations"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(statement)
        .any(|statement| {
            let named: BTreeSet<(&str, &str)> = statement
                .get("subject")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|subject| {
                    Some((
                        subject.get("name")?.as_str()?,
                        subject.pointer("/digest/sha512")?.as_str()?,
                    ))
                })
                .collect();
            named == expected
        })
}

/// The in-toto statement an attestation's DSSE envelope carries.
fn statement(attestation: &Value) -> Option<Value> {
    let payload = attestation.pointer("/bundle/dsseEnvelope/payload").and_then(Value::as_str)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Reads what pnpr holds of the release, for the caller.
struct Reader<'r> {
    client: &'r ThrottledClient,
    registry: &'r NormalizedRegistryUrl,
    authorization: Option<&'r str>,
    release: &'r Release<'r>,
}

impl Reader<'_> {
    /// The JSON `path` holds on the registry, or `None` when it answers 404.
    async fn get(&self, path: &str) -> Result<Option<Value>, PublishedReleaseError> {
        let url = self.registry
            .join(path)
            .map_err(|error| self.unread(&error.to_string()))?;
        let guard = self.client.acquire_for_url(&url).await;
        let mut request = guard.get(&url).header("accept", "application/json");
        if let Some(authorization) = self.authorization {
            request = request.header("authorization", authorization);
        }
        let response = request
            .send()
            .await
            .map_err(|error| self.unread(&error.to_string()))?;
        match response.status().as_u16() {
            200 => {}
            404 => return Ok(None),
            status => return Err(self.unread(&format!("GET {path} answered {status}"))),
        }
        let body = read_limited_body(response, BODY_LIMIT).await
            .map_err(|error| self.unread(&error.to_string()))?;
        if body.truncated {
            return Err(self.unread(&format!("GET {path} answered more than {BODY_LIMIT} bytes")));
        }
        serde_json::from_slice(&body.bytes)
            .map(Some)
            .map_err(|error| self.unread(&format!("GET {path} answered invalid JSON: {error}")))
    }

    fn differs(&self, difference: String) -> PublishedReleaseError {
        PublishedReleaseError::Differs {
            package: format!("{}@{}", self.release.name, self.release.version),
            difference,
        }
    }

    fn unread(&self, reason: &str) -> PublishedReleaseError {
        PublishedReleaseError::Unread {
            package: format!("{}@{}", self.release.name, self.release.version),
            registry: self.registry.for_display(),
            reason: reason.to_string(),
        }
    }
}

/// A version pnpr reports as already published is not, or could not be
/// confirmed to be, the release this publish would have made.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum PublishedReleaseError {
    #[display("{package} is already published, and not as this release: {difference}")]
    #[diagnostic(
        code(ERR_PNPM_PUBLISHED_RELEASE_DIFFERS),
        help(
            "A published version never changes. Publish the changes as a new version, or rerun the job that published this one with the same archives and dist-tag."
        )
    )]
    Differs {
        #[error(not(source))]
        package: String,
        difference: String,
    },

    #[display(
        "{package} is already published, and reading its release from {registry} failed: {reason}"
    )]
    #[diagnostic(
        code(ERR_PNPM_PUBLISHED_RELEASE_UNREAD),
        help("Run the publish again to check the published release once more.")
    )]
    Unread {
        #[error(not(source))]
        package: String,
        registry: String,
        reason: String,
    },
}

#[cfg(test)]
mod tests;
