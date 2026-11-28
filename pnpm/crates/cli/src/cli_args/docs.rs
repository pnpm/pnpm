use clap::Args;
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_network::redact_url_for_display;
use pnpm_network_web_auth::OpenUrl;
use pnpm_registry::PackageVersion;

/// Open the documentation page of a package in a browser.
#[derive(Debug, Args)]
pub struct DocsArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// Package name (optionally with @version).
    // Any number is accepted here because a same-named script that
    // overrides the command receives them all. The built-in takes one.
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub(crate) enum DocsError {
    #[display("Exactly one package name is required")]
    #[diagnostic(code(ERR_PNPM_DOCS_PACKAGE_NAME))]
    PackageName,
}

impl DocsArgs {
    pub async fn run<Sys: OpenUrl>(self, config: &Config) -> miette::Result<()> {
        let url = self.documentation_url(config).await?;
        open_url::<Sys>(&url)
    }

    async fn documentation_url(&self, config: &Config) -> miette::Result<String> {
        let [package] = self.packages.as_slice() else {
            return Err(DocsError::PackageName.into());
        };
        let fetched =
            super::view::fetch_package_metadata(config, self.registry.as_deref(), package, "docs")
                .await?;
        Ok(documentation_url_from_manifest(&fetched.picked))
    }
}

fn documentation_url_from_manifest(manifest: &PackageVersion) -> String {
    manifest.other
        .get("homepage")
        .and_then(serde_json::Value::as_str)
        .filter(|homepage| is_http_url(homepage))
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("https://npmx.dev/package/{}", manifest.name))
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
            let redacted = redact_url_for_display(url);
            pnpm_diagnostics::eprintln!("Could not open browser: {e}");
            pnpm_diagnostics::println!("{redacted}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
