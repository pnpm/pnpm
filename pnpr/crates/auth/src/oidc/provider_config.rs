use super::{
    AsyncMutex, HashMap, HashSet, MetadataCache, OidcProvider, Provider, Result, Url,
    invalid_config,
};

pub(super) fn validate_provider(config: &OidcProvider) -> Result<()> {
    if config.name.is_empty()
        || config.name.len() > 64
        || !config
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        || config.audience.is_empty()
    {
        return Err(invalid_config(
            "OIDC providers require a name (letters, digits, '-' or '_') and audience",
        ));
    }
    secure_url(&config.issuer)?;
    let mut subjects = HashSet::new();
    for binding in config
        .login
        .iter()
        .flat_map(|login| &login.users)
        .chain(config.workloads.iter().map(|workload| &workload.identity))
    {
        super::super::validate_username(&binding.username)
            .map_err(|_| invalid_config("invalid OIDC username"))?;
        if binding.subject.is_empty() || !subjects.insert(&binding.subject) {
            return Err(invalid_config(
                "OIDC subjects must be nonempty and unique within each provider",
            ));
        }
    }
    if config.login.as_ref().is_some_and(|login| login.users.is_empty())
        || (config.login.is_none() && config.workloads.is_empty())
    {
        return Err(invalid_config("OIDC providers require explicit user or workload bindings"));
    }
    Ok(())
}

pub(super) fn secure_url(raw: &str) -> Result<()> {
    let url = Url::parse(raw).map_err(|_| invalid_config("invalid OIDC URL"))?;
    let secure = url.scheme() == "https";
    #[cfg(test)]
    let secure = secure || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1"));
    if !secure
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_config(
            "OIDC URLs require HTTPS without credentials, query, or fragment",
        ));
    }
    Ok(())
}

pub(super) fn build_providers(
    configs: &[OidcProvider],
    public_url: &str,
) -> Result<HashMap<String, Provider>> {
    let mut providers = HashMap::new();
    for config in configs {
        validate_provider(config)?;
        if providers
            .insert(
                config.name.clone(),
                Provider {
                    config: config.clone(),
                    metadata: AsyncMutex::new(MetadataCache::default()),
                    refresh: AsyncMutex::new(()),
                },
            )
            .is_some()
        {
            return Err(invalid_config("duplicate OIDC provider name"));
        }
        if config.login.is_some() {
            secure_url(public_url)?;
            let url = Url::parse(public_url).map_err(|_| invalid_config("invalid public URL"))?;
            if url.path() != "/" && !url.path().is_empty() {
                return Err(invalid_config("OIDC login requires --public-url at the origin root"));
            }
        }
    }
    Ok(providers)
}
