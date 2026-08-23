//! Publish several packed packages through pnpr's atomic batch endpoint.

use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_network_web_auth::{Host as WebAuthHost, WithOtpError};
use pnpm_reporter::Reporter;
use serde_json::Value;

use crate::{
    failed_to_publish_error::FailedToPublishError,
    global_log::{global_info, global_warn},
    publish_options::{
        PublishUnsupportedRegistryProtocolError, find_registry_info, resolve_access,
    },
    publish_packed_pkg::{
        DistHashes, PackedPkg, PublishHttpError, PublishNetwork, PublishPackedPkgError,
        PublishPackedPkgOptions, build_publish_document, join_registry, publish_with_otp_handling,
        registry_for_display, web_auth_fetch_options,
    },
    publish_summary::{PackedPkgInfo, PublishSummary, create_publish_summary},
    registry_config_keys::NormalizedRegistryUrl,
};

const BATCH_PUBLISH_ENDPOINT: &str = "-/pnpm/v1/publish";

struct BatchGroup {
    registry: NormalizedRegistryUrl,
    package_names: Vec<String>,
    summary_indexes: Vec<usize>,
    documents: Vec<Value>,
}

/// Reject publish modes whose per-package artifacts cannot be represented by
/// one batch request.
pub fn validate_batch_publish_options(
    opts: &PublishPackedPkgOptions,
) -> Result<(), BatchPublishError> {
    if opts.stage {
        return Err(BatchPublishError::Stage);
    }
    if opts.provenance == Some(true) {
        return Err(BatchPublishError::Provenance);
    }
    Ok(())
}

/// Publish every packed package in one request per target registry.
pub async fn batch_publish_packed_pkgs<Reporter>(
    packages: &[PackedPkg<'_>],
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
) -> Result<Vec<PublishSummary>, BatchPublishError>
where
    Reporter: self::Reporter,
{
    validate_batch_publish_options(opts)?;

    let mut summaries = Vec::with_capacity(packages.len());
    let mut groups: Vec<BatchGroup> = Vec::new();
    for package in packages {
        let manifest = package.published_manifest;
        let name = manifest.get("name").and_then(Value::as_str).unwrap_or_default();
        let publish_config_registry = manifest
            .get("publishConfig")
            .and_then(|config| config.get("registry"))
            .and_then(Value::as_str);
        let registry = find_registry_info(
            name,
            &opts.default_registry,
            &opts.scoped_registries,
            publish_config_registry,
        )?;
        let summary = create_publish_summary(
            &PackedPkgInfo {
                published_manifest: manifest,
                tarball_path: package.tarball_path,
                contents: package.contents,
                unpacked_size: package.unpacked_size,
            },
            package.tarball_data,
        );
        let document = build_publish_document(
            manifest,
            package.tarball_data,
            &registry,
            resolve_access(opts.access, manifest),
            &opts.tag,
            &DistHashes { integrity: &summary.integrity, shasum: &summary.shasum },
        )?;
        let summary_index = summaries.len();
        summaries.push(summary);

        if let Some(group) = groups.iter_mut().find(|group| group.registry == registry) {
            group.package_names.push(name.to_string());
            group.summary_indexes.push(summary_index);
            group.documents.push(document);
        } else {
            groups.push(BatchGroup {
                registry,
                package_names: vec![name.to_string()],
                summary_indexes: vec![summary_index],
                documents: vec![document],
            });
        }
    }

    for group in groups {
        let registry = registry_for_display(&group.registry);
        for &summary_index in &group.summary_indexes {
            global_info::<Reporter>(&format!("📦 {} → {registry}", summaries[summary_index].id));
        }
        if opts.dry_run {
            global_warn::<Reporter>(&format!(
                "Skip publishing {} package(s) to {registry} (dry run)",
                group.documents.len(),
            ));
            continue;
        }

        let put_url = join_registry(&group.registry, BATCH_PUBLISH_ENDPOINT)?;
        let authorization = network.auth_headers.for_url_with_package(
            group.registry.as_str(),
            group.package_names.first().map(String::as_str),
        );
        let body = bytes::Bytes::from(
            serde_json::to_vec(&serde_json::json!({ "packages": group.documents }))
                .expect("serialize batch publish documents"),
        );
        let response = publish_with_otp_handling::<WebAuthHost, Reporter>(
            network.client,
            &put_url,
            authorization.as_deref(),
            "publish",
            body,
            opts.otp.as_deref(),
            false,
            web_auth_fetch_options(&opts.http),
        )
        .await?;
        if !response.ok {
            if matches!(response.status, 404 | 405) {
                return Err(BatchPublishError::Unsupported { registry });
            }
            return Err(BatchPublishError::Failed(FailedToPublishError::new_batch(
                group.package_names.len(),
                &registry,
                response.status,
                response.status_text,
                response.body,
            )));
        }
        global_info::<Reporter>(&format!(
            "✅ Published {} package(s) to {registry} in a single request",
            group.summary_indexes.len(),
        ));
    }

    Ok(summaries)
}

/// Failures specific to batch publishing.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum BatchPublishError {
    #[display("Staged publishing cannot be combined with --batch")]
    #[diagnostic(code(ERR_PNPM_BATCH_PUBLISH_NO_STAGE))]
    Stage,

    #[display("Provenance statements cannot be generated when publishing with --batch")]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_NO_PROVENANCE),
        help(
            "Provenance is bound to a single package, but --batch sends many packages in one request. Publish without --batch to attach provenance."
        )
    )]
    Provenance,

    #[display(
        "The registry at {registry} does not support publishing multiple packages in a single request"
    )]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED),
        help(
            r#"Retry without the --batch flag, or publish to a registry that implements "PUT /-/pnpm/v1/publish" (for example, pnpr)."#
        )
    )]
    Unsupported {
        #[error(not(source))]
        registry: String,
    },

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Registry(PublishUnsupportedRegistryProtocolError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Package(PublishPackedPkgError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Otp(WithOtpError<PublishHttpError>),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Failed(FailedToPublishError),
}

impl From<PublishUnsupportedRegistryProtocolError> for BatchPublishError {
    fn from(error: PublishUnsupportedRegistryProtocolError) -> Self {
        BatchPublishError::Registry(error)
    }
}

impl From<PublishPackedPkgError> for BatchPublishError {
    fn from(error: PublishPackedPkgError) -> Self {
        BatchPublishError::Package(error)
    }
}

impl From<WithOtpError<PublishHttpError>> for BatchPublishError {
    fn from(error: WithOtpError<PublishHttpError>) -> Self {
        BatchPublishError::Otp(error)
    }
}
