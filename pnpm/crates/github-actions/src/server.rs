/// Resolves the effective GitHub server base URL: the
/// `update.githubActionsServer` setting, the `GITHUB_SERVER_URL`
/// environment variable, or <https://github.com> — first non-empty wins.
pub(super) fn resolve_server_url(server_url: Option<&str>) -> miette::Result<String> {
    let url = server_url
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("GITHUB_SERVER_URL")
                .ok()
                .filter(|url| !url.is_empty())
        })
        .unwrap_or_else(|| "https://github.com".to_string());
    validate_server_url(&url)
}

pub(super) fn validate_server_url(url: &str) -> miette::Result<String> {
    let parsed = url::Url::parse(url)
        .ok()
        .filter(|parsed| {
            parsed.host_str().is_some()
                && pnpm_network::is_url_secure_for_credentials(parsed.as_str())
        });
    let Some(parsed) = parsed else {
        return Err(miette::miette!(
            code = "ERR_PNPM_GITHUB_ACTIONS_SERVER_PROTOCOL",
            "The GitHub Actions server URL must use HTTPS, except for HTTP on loopback hosts",
        ));
    };
    Ok(parsed
        .as_str()
        .trim_end_matches('/')
        .to_string())
}
