use super::{
    Arc, AuditSettings, BTreeMap, BUILTIN_REGISTRIES_BY_PREFIX, Config, DEFAULT_JSR_REGISTRY,
    NeedsFullMetadataFor, RegistryDeclaration, RegistryLookups, ResolutionMode, UpdateSettings,
    full_metadata_policy, registries,
};

impl Config {
    /// The resolved retry policy for metadata and artifact requests.
    #[must_use]
    pub fn retry_opts(&self) -> pnpm_network::RetryOpts {
        pnpm_network::RetryOpts {
            retries: self.fetch_retries,
            factor: self.fetch_retry_factor,
            min_timeout: std::time::Duration::from_millis(self.fetch_retry_mintimeout),
            max_timeout: std::time::Duration::from_millis(self.fetch_retry_maxtimeout),
        }
    }

    /// The resolved settings used to construct an install HTTP client.
    #[must_use]
    pub fn network_settings(&self) -> pnpm_network::NetworkSettings {
        pnpm_network::NetworkSettings {
            network_concurrency: self.network_concurrency,
            fetch_timeout: std::time::Duration::from_millis(self.fetch_timeout),
            fetch_warn_timeout: std::time::Duration::from_millis(self.fetch_warn_timeout_ms),
            fetch_min_speed_ki_bps: self.fetch_min_speed_ki_bps,
            user_agent: self.user_agent.clone(),
        }
    }

    /// The registries this config declares, in the shape the `registries`
    /// setting is written in — what a pnpr server is told about them.
    ///
    /// The default registry is not among them: it travels as the request's
    /// own `registry` field.
    #[must_use]
    pub fn registry_declarations(&self) -> BTreeMap<String, RegistryDeclaration> {
        registries::to_declarations(&self.registry_lookups(None))
    }

    /// The registries this config resolves from, merged across every source,
    /// in the shape the `registries` setting is written in — the view
    /// `pnpm config get registries` prints. Unlike
    /// [`Self::registry_declarations`], nothing is omitted: the default
    /// registry is declared as the bare `@` scope, and the built-in routes —
    /// the `@jsr` scope and the [`BUILTIN_REGISTRIES_BY_PREFIX`] prefixes —
    /// are declared too, unless the user pointed them elsewhere.
    #[must_use]
    pub fn resolved_registry_declarations(&self) -> BTreeMap<String, RegistryDeclaration> {
        registries::to_resolved_declarations(&self.resolved_registry_lookups())
    }

    /// The scope and prefix routes the CLI resolves a package's registry
    /// through, including the built-in ones pnpm answers without being
    /// told: the `@jsr` scope and the [`BUILTIN_REGISTRIES_BY_PREFIX`]
    /// prefixes, unless the user pointed them elsewhere.
    #[must_use]
    pub fn resolved_registry_lookups(&self) -> RegistryLookups {
        let mut lookups = self.registry_lookups(Some(self.registry.clone()));
        lookups
            .registries_by_scope
            .entry("@jsr".to_string())
            .or_insert_with(|| DEFAULT_JSR_REGISTRY.to_string());
        for (prefix, registry) in BUILTIN_REGISTRIES_BY_PREFIX {
            lookups
                .registries_by_prefix
                .entry((*prefix).to_string())
                .or_insert_with(|| (*registry).to_string());
        }
        lookups
    }

    pub(super) fn registry_lookups(&self, default_registry: Option<String>) -> RegistryLookups {
        RegistryLookups {
            registries_by_scope: self.registries_by_scope.clone(),
            default_registry,
            registries_by_prefix: self.registries_by_prefix.clone(),
            registry_options_by_url: self.registry_options_by_url.clone(),
        }
    }

    /// The `update` settings the CLI acts on, re-joined from
    /// [`Self::update_config`] — the view `pnpm config get update` prints.
    /// `None` when nothing is set.
    #[must_use]
    pub fn resolved_update_settings(&self) -> Option<UpdateSettings> {
        let update = UpdateSettings {
            ignore_deps: self.update_config.ignore_dependencies.clone(),
            changeset: self.update_config.changeset,
            github_actions: self.update_config.github_actions,
            github_actions_server: self.update_config.github_actions_server.clone(),
        };
        (update != UpdateSettings::default()).then_some(update)
    }

    /// The `audit` settings the CLI acts on, re-joined from
    /// [`Self::audit_level`] and [`Self::audit_config`] — the view
    /// `pnpm config get audit` prints. An empty ignore list reads as unset.
    /// `None` when nothing is set.
    #[must_use]
    pub fn resolved_audit_settings(&self) -> Option<AuditSettings> {
        let audit = AuditSettings {
            level: self.audit_level,
            ignore: (!self.audit_config.ignore_ghsas.is_empty())
                .then(|| self.audit_config.ignore_ghsas.clone()),
            ignore_prune: self.audit_ignore_prune,
        };
        (audit != AuditSettings::default()).then_some(audit)
    }

    /// Overlay the CLI's proxy flags onto the merged keys and re-resolve.
    ///
    /// Only the empty string reads as unset here. A flag carries its value
    /// verbatim, so it has none of the scalar typing that turns a `false` or
    /// `null` in an `.npmrc` or yaml into a non-string; on the command line
    /// those are ordinary hostnames.
    pub fn apply_proxy_cli_overrides(
        &mut self,
        https_proxy: Option<&str>,
        http_proxy: Option<&str>,
        no_proxy: Option<&str>,
    ) {
        for (proxy, keys) in [
            (&mut self.proxy, &mut self.proxy_keys),
            (
                &mut self.package_manager_bootstrap.proxy,
                &mut self.package_manager_bootstrap.proxy_keys,
            ),
        ] {
            for (key, raw) in [
                (&mut keys.https_proxy, https_proxy),
                (&mut keys.http_proxy, http_proxy),
                (&mut keys.no_proxy, no_proxy),
            ] {
                if let Some(raw) = raw {
                    *key = crate::proxy_keys::ProxyValue::from_flag(raw);
                }
            }
            *proxy = keys.resolve();
        }
    }

    /// Effective value of [`Self::minimum_release_age_strict`]: the
    /// user-supplied value when set, otherwise `true` if `minimumReleaseAge`
    /// was explicitly configured.
    ///
    /// Without that default a user-set cutoff would silently fall back to an
    /// immature version whenever no mature one satisfies the range, making the
    /// setting look like it had no effect. The built-in 1440-minute default
    /// stays non-strict for backward compatibility, so the two are told apart
    /// through [`Self::explicit_settings`] rather than through the value
    /// itself. A repository's cutoff never reaches `self-update`, which
    /// [`WorkspaceSettings::clear_self_update_policy`] drops before the
    /// workspace yaml is recorded there.
    ///
    /// [`WorkspaceSettings::clear_self_update_policy`]: crate::WorkspaceSettings::clear_self_update_policy
    pub fn resolved_minimum_release_age_strict(&self) -> bool {
        self.minimum_release_age_strict
            .unwrap_or_else(|| self.explicit_settings.contains_key("minimumReleaseAge"))
    }

    /// Effective [`Self::minimum_release_age`], with `Some(0)` treated
    /// as "disabled" (`None`).
    ///
    /// A falsy check: `minimumReleaseAge: 0` disables the maturity
    /// cutoff. Disabling it is also what makes `resolutionMode:
    /// lowest-direct` / `time-based` observable for direct dependencies
    /// — while a cutoff is active the picker always prefers the highest
    /// mature version, overriding the lowest-version pick.
    pub fn resolved_minimum_release_age(&self) -> Option<u64> {
        self.minimum_release_age.filter(|&minutes| minutes > 0)
    }

    /// Whether version resolution must fetch the full packument to obtain
    /// per-version `time` and trust evidence.
    ///
    /// The `no-downgrade` trust check reads per-version trust evidence
    /// (`_npmUser` / `dist.attestations`) that the abbreviated packument
    /// *never* carries — `registrySupportsTimeField` only concerns the
    /// `time` field — so it always requires the full packument. Time-based
    /// resolution needs only `time`, which abbreviated metadata carries
    /// when the registry advertises it, so it is gated on
    /// `!registrySupportsTimeField`.
    ///
    /// `minimumReleaseAge` is intentionally absent: the resolver upgrades
    /// abbreviated metadata to full on demand for the maturity check (see
    /// `maybe_upgrade_abbreviated_meta_for_release_age`), so it doesn't
    /// need the full packument requested up front.
    ///
    /// The install resolver (`PickPolicy`), `pacquet add`'s pre-resolution,
    /// and the `self-update` / `pnpm with` engine probe all derive their
    /// metadata mode from here so none of them can drift.
    #[must_use]
    pub fn requires_full_metadata_for_resolution(&self) -> bool {
        self.full_metadata_policy(self.registry_supports_time_field)
    }

    /// The same policy, asked of one registry: a registry that declares
    /// `supportsTimeField` answers for itself, so a time-based resolution
    /// reads abbreviated metadata from the registries that carry `time` and
    /// full metadata only from the ones that do not.
    ///
    /// A registry with no declaration answers exactly what
    /// [`Self::requires_full_metadata_for_resolution`] does.
    #[must_use]
    pub fn requires_full_metadata_for_registry(&self, registry: &str) -> bool {
        self.full_metadata_policy(self.registry_supports_time_field(registry))
    }

    /// Whether a full packument, once fetched, is stored and read in pnpm's
    /// filtered form rather than verbatim.
    ///
    /// Answered for the most demanding registry — one that carries no `time`
    /// — because [`Self::requires_full_metadata_for_registry`] can ask for a
    /// full document at a registry
    /// [`Self::requires_full_metadata_for_resolution`] would have left on
    /// abbreviated metadata, and both have to agree on which mirror that
    /// document lands in. It is consulted only when a full document is
    /// actually fetched, so answering for the demanding case costs the others
    /// nothing.
    #[must_use]
    pub fn requires_filtered_full_metadata(&self) -> bool {
        self.full_metadata_policy(false)
    }

    /// Whether `registry`'s abbreviated metadata carries the `time` field,
    /// from its own declaration if it has one and from the
    /// `registrySupportsTimeField` setting otherwise.
    #[must_use]
    pub fn registry_supports_time_field(&self, registry: &str) -> bool {
        pnpm_lockfile::registry_supports_time_field(&self.registry_options_by_url, registry)
            .unwrap_or(self.registry_supports_time_field)
    }

    pub(super) fn full_metadata_policy(&self, supports_time_field: bool) -> bool {
        full_metadata_policy(
            self.trust_policy,
            self.resolution_mode == ResolutionMode::TimeBased,
            supports_time_field,
        )
    }

    /// [`Self::requires_full_metadata_for_registry`] as a closure the resolver
    /// can hold, capturing the four facts it needs rather than the config.
    #[must_use]
    pub fn requires_full_metadata_for_registry_fn(&self) -> NeedsFullMetadataFor {
        let registry_options_by_url = self.registry_options_by_url.clone();
        let default_supports_time_field = self.registry_supports_time_field;
        let trust_policy = self.trust_policy;
        let time_based = self.resolution_mode == ResolutionMode::TimeBased;
        Arc::new(move |registry: &str| {
            let supports_time_field =
                pnpm_lockfile::registry_supports_time_field(&registry_options_by_url, registry)
                    .unwrap_or(default_supports_time_field);
            full_metadata_policy(trust_policy, time_based, supports_time_field)
        })
    }

    /// Registry map in pnpm's `Registries` shape: `default` plus the
    /// configured scoped routes keyed by `@scope`.
    ///
    /// The built-in `@jsr` route is one of them, so every consumer that
    /// routes a package by its scope — the resolver, the lockfile
    /// verifier, `pnpm why`, `pnpm view` — reaches JSR packages at
    /// npm.jsr.io instead of asking the default registry for an
    /// `@jsr/*` packument it does not serve. A configured `@jsr:registry`
    /// wins over it.
    #[must_use]
    pub fn resolved_registries(&self) -> BTreeMap<String, String> {
        let mut registries = self.registries_by_scope.clone();
        registries.entry("@jsr".to_string()).or_insert_with(|| DEFAULT_JSR_REGISTRY.to_string());
        registries.insert("default".to_string(), self.registry.clone());
        registries
    }
}
