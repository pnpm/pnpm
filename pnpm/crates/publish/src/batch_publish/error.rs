//! The ways a batch publish fails.

use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_network_web_auth::WithOtpError;

use crate::{
    failed_to_publish_error::FailedToPublishError,
    publish_options::PublishUnsupportedRegistryProtocolError,
    publish_packed_pkg::{PublishHttpError, PublishPackedPkgError},
    publish_wait::PublishWaitError,
    staged_archives::StagingError,
};

/// Failures specific to batch publishing.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum BatchPublishError {
    #[diagnostic(transparent)]
    Wait(PublishWaitError),
    #[display("Staged publishing cannot be combined with --batch")]
    #[diagnostic(code(ERR_PNPM_BATCH_PUBLISH_NO_STAGE))]
    Stage,

    #[display(
        "The batch for {registry} references {count} archives, more than one publish to it may"
    )]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_TOO_MANY_ARCHIVES),
        help(
            "A version with platform variants is published from staged archives, which one publish references at most 256 of. Publish fewer packages per batch."
        )
    )]
    TooManyArchives {
        #[error(not(source))]
        registry: String,
        count: usize,
    },

    #[display("Provenance statements cannot be generated when publishing with --batch")]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_NO_PROVENANCE),
        help(
            "Provenance is bound to a single package, but --batch sends many packages in one request. Publish without --batch to attach provenance."
        )
    )]
    Provenance,

    #[display(
        "Packages targeting {registry} resolve to different authentication credentials and cannot be published in one batch"
    )]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_CONFLICTING_CREDENTIALS),
        help(
            "Configure one credential that can publish every package targeting this registry, or publish without --batch."
        )
    )]
    ConflictingCredentials {
        #[error(not(source))]
        registry: String,
    },

    #[display(
        "The registry at {registry} does not support publishing multiple packages in a single request"
    )]
    #[diagnostic(
        code(ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED),
        help(
            r#"Retry without the --batch flag, or publish to a registry that implements "PUT /-/pnpr/v0/publish" (for example, pnpr)."#
        )
    )]
    Unsupported {
        #[error(not(source))]
        registry: String,
    },

    #[diagnostic(transparent)]
    Registry(PublishUnsupportedRegistryProtocolError),

    #[diagnostic(transparent)]
    Package(PublishPackedPkgError),

    #[diagnostic(transparent)]
    Otp(WithOtpError<PublishHttpError>),

    #[diagnostic(transparent)]
    Failed(FailedToPublishError),

    #[diagnostic(transparent)]
    Staging(StagingError),
}

impl From<StagingError> for BatchPublishError {
    fn from(error: StagingError) -> Self {
        BatchPublishError::Staging(error)
    }
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
