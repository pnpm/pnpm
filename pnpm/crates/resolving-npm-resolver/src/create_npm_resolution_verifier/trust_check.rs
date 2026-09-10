use super::{
    JsonValue, NpmResolutionVerifier, PkgName, ResolutionVerification,
    TRUST_DOWNGRADE_VIOLATION_CODE, TrustCheckOptions, TrustPolicy, fail_if_trust_downgraded,
    format_trust_violation,
};

impl NpmResolutionVerifier {
    /// Every unconditional binding must have been checked under the same
    /// named-registry routing map before a cached verification can be trusted.
    pub(super) fn past_check_has_structural_rules(
        &self,
        cached_policy: &serde_json::Map<String, JsonValue>,
    ) -> bool {
        let recorded_every_unconditional_rule =
            ["tarballUrlBinding", "revisionHistoryBinding", "integrityRequired"]
                .into_iter()
                .all(|flag| cached_policy.get(flag).and_then(JsonValue::as_bool) == Some(true));
        if !recorded_every_unconditional_rule {
            return false;
        }

        if cached_policy.get("namedRegistriesRouting")
            != self.policy_snapshot.get("namedRegistriesRouting")
        {
            return false;
        }

        true
    }

    pub(super) fn trust_check_active(&self) -> bool {
        matches!(self.trust_policy, Some(TrustPolicy::NoDowngrade))
    }

    pub(super) fn trust_policy_wire_str(&self) -> Option<&'static str> {
        match self.trust_policy {
            Some(TrustPolicy::NoDowngrade) => Some("no-downgrade"),
            Some(TrustPolicy::Off) | None => None,
        }
    }

    /// Run the resolver-time `failIfTrustDowngraded` check against the
    /// pinned lockfile version.
    pub(super) async fn run_trust_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<ResolutionVerification> {
        let meta = match self.fetch_full_meta_for_trust(registry, name).await {
            Ok(meta) => meta,
            // A transport failure propagates the registry's own fetch error so
            // the install aborts with it rather than folding it into a policy
            // violation.
            Err(message) => return Some(ResolutionVerification::FetchFailed { message }),
        };
        let trust_opts = TrustCheckOptions {
            trust_policy_exclude: self.trust_policy_exclude.as_ref(),
            trust_policy_ignore_after_minutes: self.trust_policy_ignore_after,
            now: self.now,
            ignore_missing_time_field: self.ignore_missing_time_field,
        };
        match fail_if_trust_downgraded(&meta, version, &trust_opts) {
            Ok(()) => None,
            Err(err) => Some(ResolutionVerification::Err {
                code: TRUST_DOWNGRADE_VIOLATION_CODE,
                reason: format_trust_violation(err),
            }),
        }
    }
}
