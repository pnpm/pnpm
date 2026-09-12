use clap::Args;
use pnpm_config::Config;
use pnpm_network_web_auth::OpenUrl;
use pnpm_registry::PackageVersion;

/// Open the documentation page of a package in a browser.
#[derive(Debug, Args)]
pub struct DocsArgs {
    /// Package name (optionally with @version).
    pub package: String,
}

impl DocsArgs {
    pub async fn run<Sys: OpenUrl>(self, config: &Config) -> miette::Result<()> {
        let url = self.documentation_url(config).await?;
        open_url::<Sys>(&url)
    }

    async fn documentation_url(&self, config: &Config) -> miette::Result<String> {
        let (_, manifest) =
            super::view::fetch_package_metadata(config, None, &self.package, "docs").await?;
        Ok(documentation_url_from_manifest(&manifest))
    }
}

fn documentation_url_from_manifest(manifest: &PackageVersion) -> String {
    manifest
        .other
        .get("homepage")
        .and_then(serde_json::Value::as_str)
        .filter(|homepage| is_http_url(homepage))
        .map_or_else(|| format!("https://npmx.dev/package/{}", manifest.name), ToString::to_string)
}

fn is_http_url(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    url::Url::parse(value)
        .is_ok_and(|parsed| parsed.scheme() == "http" || parsed.scheme() == "https")
}

fn open_url<Sys: OpenUrl>(url: &str) -> miette::Result<()> {
    match Sys::open_url(url) {
        Ok(()) => Ok(()),
        Err(e) => {
            let redacted = url::Url::parse(url).map_or_else(
                |_| url.to_string(),
                |mut parsed_url| {
                    let _ = parsed_url.set_username("");
                    let _ = parsed_url.set_password(None);
                    parsed_url.to_string()
                },
            );
            eprintln!("Could not open browser: {e}");
            println!("{redacted}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
