use crate::profile::Profile;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What a registry statement claims about a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredicateType {
    /// The registry accepted the binding of the record's manifest to its
    /// archive under its profile.
    Acceptance,
    /// How the artifact was published.
    Publication,
    /// How an OCI tag came to point where its record says.
    TagBinding,
}

impl PredicateType {
    /// The predicate type URI.
    #[must_use]
    pub const fn uri(self) -> &'static str {
        match self {
            PredicateType::Acceptance => "https://pnpm.io/pnpr/acceptance/v1",
            PredicateType::Publication => "https://pnpm.io/pnpr/publication/v1",
            PredicateType::TagBinding => "https://pnpm.io/pnpr/tag-binding/v1",
        }
    }

    /// The predicate type a URI names.
    #[must_use]
    pub fn from_uri(uri: &str) -> Option<Self> {
        [PredicateType::Acceptance, PredicateType::Publication, PredicateType::TagBinding]
            .into_iter()
            .find(|predicate_type| predicate_type.uri() == uri)
    }

    /// Whether a registry makes this claim about records of `profile`.
    /// Acceptance and publication describe released artifacts, so OCI tag
    /// records, tombstones included, get only tag bindings.
    #[must_use]
    pub fn applies_to(self, profile: &Profile) -> bool {
        match self {
            PredicateType::Acceptance | PredicateType::Publication => !profile.is_oci_tag(),
            PredicateType::TagBinding => profile.is_oci_tag(),
        }
    }
}

/// The predicate of an acceptance statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptancePredicate {
    /// The profile the registry accepted the record under.
    pub interpretation: Profile,
}

/// How a tag binding came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagAction {
    /// A push to a hosted repository set the tag.
    Set,
    /// A deletion of the tag or of its manifest removed it.
    Removed,
    /// The registry observed the binding upstream, for a proxied repository.
    Observed,
}

/// The predicate of a tag binding statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagBindingPredicate {
    /// How the binding came about.
    pub action: TagAction,
    /// The principal that pushed or removed the tag, for a hosted repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
}

/// The kind of principal that published an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalKind {
    /// A long-lived user token.
    UserToken,
    /// An interactive login session.
    InteractiveSession,
    /// An OIDC workload, such as a CI job.
    OidcWorkload,
}

/// The OIDC workload that published an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workload {
    /// The token issuer.
    pub issuer: String,
    /// The token subject.
    pub subject: String,
    /// The claims the workload binding matched, such as the repository and
    /// workflow.
    #[serde(default)]
    pub claims: Map<String, Value>,
}

/// The predicate of a publication statement: the introducing publication
/// record, plus how its principal authenticated.
///
/// Only the registry produces this predicate. Publisher-supplied fields with
/// these names are stripped, and imported historical publications never
/// produce one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationPredicate {
    /// How the principal authenticated.
    pub principal_kind: PrincipalKind,
    /// The workload, for an OIDC workload publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    /// Whether the version was staged before it was released.
    pub staged: bool,
    /// The principal that approved a staged version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<String>,
    /// The fields of the introducing publication record.
    #[serde(flatten)]
    pub publication: Map<String, Value>,
}
