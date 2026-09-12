use super::{
    Config, Registry, RegistryError, org_collision_error, registry_err, validate_org_namespace,
    validate_registry_key,
};

impl Config {
    /// At least one top-level surface must be served; a server with no
    /// registry surface (no registries declared, or `--disable-registry`) and
    /// the resolver disabled would answer only `/-/ping` and the account
    /// endpoints. Checked at config load and again in the serve/router
    /// entry points for programmatically built configs.
    pub fn ensure_a_feature_is_enabled(&self) -> Result<(), RegistryError> {
        if self.registry.enabled
            || self.resolver.enabled
            || self.artifacts.enabled
            || self.pipeline.enabled
        {
            Ok(())
        } else {
            Err(RegistryError::InvalidConfig {
                reason: "nothing to serve: the npm-registry surface is off (no `registries:` \
                         declared, or `--disable-registry`), the resolver is disabled, and \
                         artifacts and the pipeline surface are disabled"
                    .to_string(),
            })
        }
    }

    /// Ready the registry graph for serving: fold every upstream into the graph
    /// as a pattern-less upstream registry, then apply every invariant YAML
    /// loading enforces — URL-safe registry names, path-safe and collision-free
    /// hosted `org` namespaces, and the graph validation itself. This covers
    /// embedders that build [`Self::upstreams`], [`Self::hosted`], or
    /// [`Self::registries`] programmatically, so [`pnpr_registry::Registries::resolve`] is the
    /// only dispatch table for `/~<name>/` traffic and a programmatically-built
    /// config fails closed like a YAML load. An embedder that wants a
    /// namespace bound on an upstream declares its registry entry (with
    /// patterns) before serving.
    pub fn ensure_valid_registry_graph(&mut self) -> Result<(), RegistryError> {
        self.ensure_upstreams_in_graph()?;
        self.ensure_concrete_registries_are_served()?;
        self.ensure_hosted_rows_match_graph()?;
        self.ensure_public_upstreams_send_no_headers()?;
        self.registries.validate().map_err(|err| registry_err(&err))
    }

    /// Every serving upstream is reachable in the graph under its own name.
    ///
    /// The fold never overwrites, so a graph entry already declared under this
    /// name must actually be the upstream — otherwise two different origins
    /// would share one `/~<name>/` identity, with the dormant upstream's
    /// credential still offered by the resolver. YAML loading rejects the same
    /// collision while building the graph.
    pub(super) fn ensure_upstreams_in_graph(&mut self) -> Result<(), RegistryError> {
        for name in self.upstreams.keys() {
            self.registries.ensure_upstream(name);
            if !matches!(self.registries.get(name), Some(Registry::Upstream { .. })) {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "upstream registry {name:?} collides with a non-upstream registry of \
                         the same name",
                    ),
                });
            }
        }
        Ok(())
    }

    /// Every concrete registry has the serving config it needs.
    ///
    /// A hosted graph entry without its `hosted` table row (or an upstream
    /// without its serving entry) would answer every request not-found at
    /// runtime. YAML loading builds both sides together; this catches a
    /// programmatically-built mismatch at startup. Upstream backing is only
    /// required when the registry surface is enabled: a resolver-only tier
    /// deliberately skips upstream (credential) resolution and never serves
    /// `/~<name>/` content.
    pub(super) fn ensure_concrete_registries_are_served(&self) -> Result<(), RegistryError> {
        for name in self.registries.names() {
            validate_registry_key(name)?;
            match self.registries.get(name) {
                Some(Registry::Hosted { .. }) if !self.hosted.contains_key(name) => {
                    return Err(RegistryError::InvalidConfig {
                        reason: format!(
                            "hosted registry {name:?} has no entry in the hosted serving table; \
                             every request to it would be not-found",
                        ),
                    });
                }
                Some(Registry::Upstream { .. })
                    if self.registry.enabled && !self.upstreams.contains_key(name) =>
                {
                    return Err(RegistryError::InvalidConfig {
                        reason: format!(
                            "upstream registry {name:?} has no serving config (URL, credentials); \
                             every request to it would fail",
                        ),
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Each hosted serving row names a hosted registry and claims an org no
    /// earlier row claimed.
    ///
    /// The mirror of the upstream collision in [`Self::ensure_upstreams_in_graph`]:
    /// a hosted serving row under a name the graph declares as a different kind
    /// would leave `/~<name>/` serving one origin while the row describes
    /// another. (A row with no graph entry at all is merely dormant.)
    pub(super) fn ensure_hosted_rows_match_graph(&self) -> Result<(), RegistryError> {
        for (index, (name, hosted)) in self.hosted.iter().enumerate() {
            validate_registry_key(name)?;
            validate_org_namespace(name, &hosted.org)?;
            if let Some(kind) = self.registries.get(name)
                && !matches!(kind, Registry::Hosted { .. })
            {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "hosted registry {name:?} collides with a non-hosted registry of the \
                         same name",
                    ),
                });
            }
            if let Some((other, _)) =
                self.hosted.iter().take(index).find(|(_, existing)| existing.org == hosted.org)
            {
                return Err(org_collision_error(name, &hosted.org, other));
            }
        }
        Ok(())
    }

    /// Mirror the YAML rule ([`resolve_upstream_registry`](crate::registry_graph::resolve_upstream_registry)): an upstream with no
    /// `access:` gate is publicly reachable at `/~<name>/`, and a public origin
    /// sends no request headers — any header can carry a credential, and an
    /// ungated endpoint would let every caller spend it (a confused deputy).
    pub(super) fn ensure_public_upstreams_send_no_headers(&self) -> Result<(), RegistryError> {
        for (name, upstream) in &self.upstreams {
            if upstream.access.is_none() && !upstream.headers.is_empty() {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "upstream registry {name:?} sends custom headers but declares no \
                         `access:` gate; a publicly reachable upstream must send none",
                    ),
                });
            }
        }
        Ok(())
    }
}
