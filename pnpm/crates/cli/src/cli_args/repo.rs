use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_network::{RetryOpts, ThrottledClient};
use pnpm_network_web_auth::OpenUrlAndWait;
use pnpm_package_manifest::{PackageManifest, PackageManifestError};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata,
    pick_registry_for_package,
};
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use std::{borrow::Cow, collections::HashMap};
use url::repository_to_web_url;

/// Opens the URL of the package's repository in a browser.
#[derive(Debug, Args)]
pub struct RepoArgs {
    /// Package names (optionally with @version) to look up.
    pub packages: Vec<String>,
}

impl RepoArgs {
    pub async fn run<Sys: OpenUrlAndWait, Rep: Reporter>(
        self,
        config: &Config,
        dir: &std::path::Path,
    ) -> miette::Result<()> {
        let prefix = dir.to_string_lossy().into_owned();

        let http_client = ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &config.network_settings(),
        )
        .into_diagnostic()
        .wrap_err("create the network client for repo")?;
        let registries: HashMap<String, String> = config
            .resolved_registries()
            .into_iter()
            .collect();

        let retry_opts = config.retry_opts();

        let urls = if self.packages.is_empty() {
            vec![get_repo_url_from_current_project(dir)?]
        } else {
            let mut urls = Vec::with_capacity(self.packages.len());
            for pkg in &self.packages {
                urls.push(
                    get_repo_url_from_registry(config, pkg, &http_client, &registries, &retry_opts)
                        .await?,
                );
            }
            urls
        };
        for url in urls {
            open_repo_url::<Sys, Rep>(&url, &prefix);
        }
        Ok(())
    }
}

/// Errors specific to `pacquet repo`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RepoError {
    #[display(
        r#"The current project does not have a repository URL. Add a "repository" field to its manifest."#
    )]
    #[diagnostic(code(ERR_PNPM_NO_REPO_URL))]
    NoRepoUrlLocal,
    #[display(r#"The package "{name}" does not have a repository URL."#)]
    #[diagnostic(code(ERR_PNPM_NO_REPO_URL))]
    NoRepoUrlRegistry { name: String },
}

fn get_repo_url_from_current_project(dir: &std::path::Path) -> miette::Result<String> {
    let manifest_path = dir.join("package.json");
    let manifest = PackageManifest::from_path(manifest_path)
        .map_err(|err| -> miette::Report {
            match &err {
                PackageManifestError::NoImporterManifestFound(_) => {
                    RepoError::NoRepoUrlLocal.into()
                }
                _ => err.into(),
            }
        })?;
    let repository = manifest.value().get("repository");
    pick_repo_url(repository).ok_or_else(|| RepoError::NoRepoUrlLocal.into())
}

async fn get_repo_url_from_registry(
    config: &Config,
    raw_spec: &str,
    http_client: &ThrottledClient,
    registries: &HashMap<String, String>,
    retry_opts: &RetryOpts,
) -> miette::Result<String> {
    let parsed = parse_wanted_dependency(raw_spec);
    let name = parsed.alias.as_deref().unwrap_or(raw_spec);
    let bare = parsed.bare_specifier.as_deref().unwrap_or("latest");
    let (resolved_name, range) = PackageManifest::resolve_registry_dependency(name, bare);

    let registry = pick_registry_for_package(registries, resolved_name, Some(bare));

    let outcome = fetch_full_metadata(
        resolved_name,
        &FetchFullMetadataOptions {
            registry: &registry,
            full_metadata: true,
            etag: None,
            modified: None,
            http: pnpm_resolving_npm_resolver::MetadataHttpClient {
                http_client,
                auth_headers: &config.auth_headers,
                retry_opts: *retry_opts,
            },
        },
    )
    .await
    .into_diagnostic()
    .wrap_err_with(|| format!("fetch package info for {raw_spec}"))?;

    let package = match outcome {
        FetchFullMetadataOutcome::Modified(pkg) => *pkg,
        FetchFullMetadataOutcome::NotModified => {
            miette::bail!("registry returned 304 Not Modified unexpectedly")
        }
    };

    let selected = select_package_version(&package, range);
    let repository = selected.and_then(|ver| ver.other.get("repository").cloned());
    pick_repo_url(repository.as_ref())
        .ok_or_else(|| {
            RepoError::NoRepoUrlRegistry {
                name: package.name.clone(),
            }
            .into()
        })
}

fn select_package_version(
    package: &pnpm_registry::Package,
    range: &str,
) -> Option<std::sync::Arc<pnpm_registry::PackageVersion>> {
    if range.is_empty() || range == "latest" {
        return package.latest();
    }
    if let Some(tag_version) = package.dist_tag(range) {
        return package.versions.get(tag_version);
    }
    package.pinned_version(range)
}

fn pick_repo_url(repository: Option<&serde_json::Value>) -> Option<String> {
    let repository = repository?;
    let (repo_url, directory) = match repository {
        serde_json::Value::String(url) => (url.clone(), None),
        serde_json::Value::Object(map) => {
            let url = map
                .get("url")?
                .as_str()?
                .to_string();
            let directory = map
                .get("directory")
                .and_then(|value| value.as_str())
                .map(String::from);
            (url, directory)
        }
        _ => return None,
    };
    repository_to_web_url(&repo_url, directory.as_deref())
}

struct HostedRepo {
    base_url: String,
    default_branch: &'static str,
}

fn redact_url(url: &str) -> String {
    ::url::Url::parse(url)
        .map_or_else(
            |_| url.to_string(),
            |mut parsed_url| {
                let _ = parsed_url.set_username("");
                let _ = parsed_url.set_password(None);
                parsed_url.set_query(None);
                parsed_url.set_fragment(None);
                parsed_url.to_string()
            },
        )
}

#[cfg(test)]
mod tests;

mod url;

fn open_repo_url<Sys: OpenUrlAndWait, Rep: Reporter>(url: &str, prefix: &str) {
    match Sys::open_url_and_wait(url) {
        Ok(()) => {}
        Err(e) => {
            let redacted = redact_url(url);
            Rep::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!("Could not open browser: {e}"),
                prefix: prefix.to_owned(),
            }));
            println!("{redacted}");
        }
    }
}
