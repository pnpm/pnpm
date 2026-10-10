//! The settings for build output shared between machines.

use super::{BTreeMap, Deserialize, overlay_some};

/// Organization-owned dependency build artifacts eligible for this workspace.
///
/// `org` and `packages` default to empty because one section is
/// assembled from several sources: the repository names the eligible
/// organization and packages while the machine supplies the trust root. The
/// feature applies only once both halves are present.
///
/// Only `org` and `packages` may come from a repository. Every other
/// field describes the act of signing and travels with the machine: loading a
/// `pnpm-workspace.yaml` that sets one fails with
/// [`LoadWorkspaceYamlError::WorkspaceRemoteSideEffectsTrust`](crate::workspace_yaml::error::LoadWorkspaceYamlError::WorkspaceRemoteSideEffectsTrust), leaving the
/// global config yaml and the environment.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror a pnpm-workspace.yaml configuration section."
    )
)]
pub struct RemoteSideEffectsCacheSettings {
    /// `org` is what pnpr calls this namespace in its own configuration and
    /// what its endpoints are built from.
    pub org: String,
    /// The alternative spelling of [`Self::org`]. A non-empty [`Self::org`]
    /// wins over this field.
    ///
    /// A separate field rather than a serde alias: an alias makes a file
    /// carrying both keys a duplicate-field parse error, where every other
    /// pair of spellings here resolves to the canonical one.
    pub organization: String,
    pub packages: Vec<String>,
    /// Publish the lifecycle-script diff of every eligible package that is built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<bool>,
    /// Identifies which of the consumer's trusted keys signed a published artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture_baseline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_env: Option<BTreeMap<String, String>>,
    /// Base64-encoded P-256 `SubjectPublicKeyInfo` DER, keyed by key id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_keys: Option<BTreeMap<String, String>>,
    /// Base64-encoded PKCS#8 P-256 private key used to sign published artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

impl RemoteSideEffectsCacheSettings {
    /// Overlay the fields `other` sets onto `self`, leaving the rest alone.
    ///
    /// A workspace declares eligibility while the machine holds the signing
    /// trust root, so the two sources contribute different fields of one
    /// section and the later one must not drop what the earlier one set.
    pub(crate) fn overlay(&mut self, other: Self) {
        let Self {
            org,
            organization,
            packages,
            publish,
            key_id,
            builder_id,
            image_digest,
            architecture_baseline,
            build_env,
            trusted_keys,
            private_key,
        } = other;
        // Resolved as the section is layered rather than at each read, so
        // that `.org` is the only spelling anything downstream has to know.
        let org = if org.is_empty() { organization } else { org };
        if !org.is_empty() {
            self.org = org;
        }
        if !packages.is_empty() {
            self.packages = packages;
        }
        overlay_some(&mut self.publish, publish);
        overlay_some(&mut self.key_id, key_id);
        overlay_some(&mut self.builder_id, builder_id);
        overlay_some(&mut self.image_digest, image_digest);
        overlay_some(&mut self.architecture_baseline, architecture_baseline);
        overlay_some(&mut self.build_env, build_env);
        overlay_some(&mut self.trusted_keys, trusted_keys);
        overlay_some(&mut self.private_key, private_key);
    }
}

/// The server that shares build output between machines, and the trust
/// every shared artifact is signed and verified with. Both the remote
/// side-effects cache and `pnpm pipeline`'s task cache read it.
///
/// `url` names a server that speaks the Turborepo Remote Cache API. Without
/// it, artifacts go to `pnprServer`.
///
/// Like [`RemoteSideEffectsCacheSettings`], one section assembled from several
/// sources: a repository may name `org`, while the server, its credential,
/// and everything that describes the act of signing travel with the machine.
/// `token` is not scoped to a URL the way `.npmrc` credentials are, so a
/// repository that could name `url` could send the token anywhere. Loading a
/// `pnpm-workspace.yaml` that sets one of those fails with
/// [`LoadWorkspaceYamlError::WorkspaceRemoteCacheTrust`](crate::workspace_yaml::error::LoadWorkspaceYamlError::WorkspaceRemoteCacheTrust).
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror a pnpm-workspace.yaml configuration section."
    )
)]
pub struct RemoteCacheSettings {
    /// The API base the `/v8/artifacts` routes hang off, such as
    /// `https://vercel.com/api`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Sent as `teamId` when it starts with `team_`, as `slug` otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// Bearer token for [`Self::url`]. Without one, the `.npmrc` credentials
    /// for the URL are used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The organization that owns the artifacts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    /// Base64-encoded P-256 `SubjectPublicKeyInfo` DER, keyed by key id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_keys: Option<BTreeMap<String, String>>,
    /// Base64-encoded PKCS#8 P-256 private key used to sign published artifacts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// Identifies which of the consumer's trusted keys signed a published artifact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builder_id: Option<String>,
    /// Publish what this machine builds. Defaults to `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture_baseline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_env: Option<BTreeMap<String, String>>,
}

impl RemoteCacheSettings {
    /// Overlay the fields `other` sets onto `self`, leaving the rest alone.
    pub(crate) fn overlay(&mut self, other: Self) {
        let Self {
            url,
            team,
            token,
            org,
            trusted_keys,
            private_key,
            key_id,
            builder_id,
            publish,
            image_digest,
            architecture_baseline,
            build_env,
        } = other;
        overlay_some(&mut self.url, url);
        overlay_some(&mut self.team, team);
        overlay_some(&mut self.token, token);
        overlay_some(&mut self.org, org);
        overlay_some(&mut self.trusted_keys, trusted_keys);
        overlay_some(&mut self.private_key, private_key);
        overlay_some(&mut self.key_id, key_id);
        overlay_some(&mut self.builder_id, builder_id);
        overlay_some(&mut self.publish, publish);
        overlay_some(&mut self.image_digest, image_digest);
        overlay_some(&mut self.architecture_baseline, architecture_baseline);
        overlay_some(&mut self.build_env, build_env);
    }

    /// Fill the signing fields `self` leaves unset from the same fields of
    /// `sideEffectsCache.remote`.
    #[must_use]
    pub fn with_side_effects_fallback(self, fallback: &RemoteSideEffectsCacheSettings) -> Self {
        let mut merged = Self {
            org: (!fallback.org.is_empty()).then(|| fallback.org.clone()),
            trusted_keys: fallback.trusted_keys.clone(),
            private_key: fallback.private_key.clone(),
            key_id: fallback.key_id.clone(),
            builder_id: fallback.builder_id.clone(),
            publish: fallback.publish,
            image_digest: fallback.image_digest.clone(),
            architecture_baseline: fallback.architecture_baseline.clone(),
            build_env: fallback.build_env.clone(),
            ..Self::default()
        };
        merged.overlay(self);
        merged
    }

    /// The fields a committed file may not set, with whether this section
    /// sets each.
    pub(crate) fn machine_only_fields(&self) -> [(&'static str, bool); 11] {
        [
            ("url", self.url.is_some()),
            ("team", self.team.is_some()),
            ("token", self.token.is_some()),
            ("trustedKeys", self.trusted_keys.is_some()),
            ("privateKey", self.private_key.is_some()),
            ("keyId", self.key_id.is_some()),
            ("builderId", self.builder_id.is_some()),
            ("publish", self.publish.is_some()),
            ("imageDigest", self.image_digest.is_some()),
            ("architectureBaseline", self.architecture_baseline.is_some()),
            ("buildEnv", self.build_env.is_some()),
        ]
    }
}
