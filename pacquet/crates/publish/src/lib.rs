//! Publish a package to an npm registry — pnpm's `publish` command,
//! implemented in Rust.

mod capabilities;
mod display_error;
mod execute_token_helper;
mod extract_manifest_from_packed;
mod failed_to_publish_error;
mod git_checks;
mod global_log;
mod oidc;
mod otp_env;
mod provenance_gen;
mod publish_options;
mod publish_packed_pkg;
mod publish_summary;
mod registry_config_keys;

pub use capabilities::{
    Clock, CommandOutput, ConfirmPrompt, EnvVar, Host, OidcFetch, OidcFetchError, OidcMethod,
    OidcRequest, OidcResponse, RunCommand,
};
pub use display_error::display_error;
pub use execute_token_helper::execute_token_helper;
pub use extract_manifest_from_packed::{
    ExtractManifestError, PublishArchiveMissingManifestError, extract_manifest_from_packed,
    is_tarball_path,
};
pub use failed_to_publish_error::FailedToPublishError;
pub use git_checks::{
    GitCheckError, get_current_branch, is_git_repo, is_remote_history_clean, is_working_tree_clean,
    run_git_checks,
};
pub use oidc::{
    AuthTokenError, DetermineProvenanceError, GetIdTokenError, IdTokenError, OidcHttpOptions,
    ProvenanceError, determine_provenance, fetch_auth_token, get_id_token,
};
pub use otp_env::resolve_otp_from_env;
pub use provenance_gen::{ProvenanceAttachment, ProvenanceGenError, generate_provenance};
pub use publish_options::{
    Access, CreatePublishOptionsError, CreatePublishOptionsInput, FetchTokenAndProvenanceError,
    OidcTokenProvenance, PublishUnsupportedRegistryProtocolError, ResolvedPublishOptions,
    create_publish_options, fetch_token_and_provenance_by_oidc, find_registry_info, resolve_access,
};
pub use publish_packed_pkg::{
    PackedPkg, PublishHttpError, PublishNetwork, PublishPackedPkgError, PublishPackedPkgOptions,
    publish_packed_pkg,
};
pub use publish_summary::{
    PackedPkgInfo, PublishSummary, PublishSummaryFile, create_publish_summary,
    extract_bundled_dependencies,
};
pub use registry_config_keys::{
    NormalizedRegistryUrl, RegistryConfigKey, SupportedRegistryUrlInfo, all_registry_config_keys,
    parse_supported_registry_url,
};
