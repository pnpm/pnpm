use super::{
    OidcState, OidcWorkload, Provider, Result, Value, match_workload_binding, rejected,
    token_payload, verify_workload,
};

impl OidcState {
    /// Verifies workload credentials against configured issuers only. The returned restrictions
    /// must be enforced before treating the mapped username as an authenticated caller.
    pub async fn workload(&self, raw: &str) -> Result<Option<OidcWorkload>> {
        if !self.providers.values().any(|provider| !provider.config.workloads.is_empty())
            || raw.split('.').count() != 3
        {
            return Ok(None);
        }
        let payload = token_payload(raw)?;
        let issuer = payload.get("iss").and_then(Value::as_str).ok_or_else(rejected)?;
        let mut matched = None;
        for provider in self.providers.values() {
            if provider.config.issuer != issuer || provider.config.workloads.is_empty() {
                continue;
            }
            if !self.verify_workload_token(provider, raw).await? {
                continue;
            }
            match_workload_binding(provider, &payload, &mut matched)?;
        }
        matched.map(Some).ok_or_else(rejected)
    }

    /// Whether the token verifies against the provider's keys, refetching its
    /// metadata once in case the signing keys have rotated.
    pub(super) async fn verify_workload_token(
        &self,
        provider: &Provider,
        raw: &str,
    ) -> Result<bool> {
        let metadata = self.metadata(provider, false).await?;
        if verify_workload(&provider.config, &metadata, raw).is_ok() {
            return Ok(true);
        }
        let refreshed = self.metadata(provider, true).await?;
        Ok(verify_workload(&provider.config, &refreshed, raw).is_ok())
    }
}
