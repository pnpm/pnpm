use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pacquet_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata,
    pick_registry_for_package,
};
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;

/// Opens the URL of the package's repository in a browser.
#[derive(Debug, Args)]
pub struct RepoArgs {
    /// Package names (optionally with @version) to look up.
    pub packages: Vec<String>,
}

impl RepoArgs {
    pub async fn run<R: Reporter>(
        self,
        config: &Config,
        dir: &std::path::Path,
    ) -> miette::Result<()> {
        let prefix = dir.to_string_lossy().into_owned();

        let http_client = ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &NetworkSettings {
                network_concurrency: config.network_concurrency,
                fetch_timeout: Duration::from_millis(config.fetch_timeout),
                user_agent: config.user_agent.clone(),
            },
        )
        .into_diagnostic()
        .wrap_err("create the network client for repo")?;

        let registries: HashMap<String, String> =
            config.resolved_registries().into_iter().collect();

        let retry_opts = RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        };

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
            match open_url(&url) {
                Ok(()) => {}
                Err(e) => {
                    let redacted = redact_url(&url);
                    R::emit(&LogEvent::Pnpm(PnpmLog {
                        level: LogLevel::Warn,
                        message: format!("Could not open browser: {e}"),
                        prefix: prefix.clone(),
                    }));
                    println!("{redacted}");
                }
            }
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
    let manifest = PackageManifest::from_path(manifest_path).map_err(|e| -> miette::Report {
        match &e {
            PackageManifestError::NoImporterManifestFound(_) => RepoError::NoRepoUrlLocal.into(),
            _ => e.into(),
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
    let bare = parsed.bare_specifier.as_deref().unwrap_or(name);
    let (resolved_name, range) = PackageManifest::resolve_registry_dependency(name, bare);

    let registry = pick_registry_for_package(registries, resolved_name, Some(bare));

    let outcome = fetch_full_metadata(
        resolved_name,
        &FetchFullMetadataOptions {
            registry: &registry,
            http_client,
            auth_headers: &config.auth_headers,
            full_metadata: true,
            etag: None,
            modified: None,
            retry_opts: *retry_opts,
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
    let repository = selected.and_then(|v| v.other.get("repository").cloned());
    pick_repo_url(repository.as_ref())
        .ok_or_else(|| RepoError::NoRepoUrlRegistry { name: package.name.clone() }.into())
}

fn select_package_version(
    package: &pacquet_registry::Package,
    range: &str,
) -> Option<std::sync::Arc<pacquet_registry::PackageVersion>> {
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
            let url = map.get("url")?.as_str()?.to_string();
            let directory = map.get("directory").and_then(|value| value.as_str()).map(String::from);
            (url, directory)
        }
        _ => return None,
    };
    repository_to_web_url(&repo_url, directory.as_deref())
}

fn repository_to_web_url(raw_url: &str, directory: Option<&str>) -> Option<String> {
    if raw_url.is_empty() {
        return None;
    }

    if let Some(url) = try_hosted_shorthand(raw_url, directory) {
        return Some(url);
    }

    let input = raw_url.strip_prefix("git+").unwrap_or(raw_url);
    let cleaned = if let Some(rest) = input.strip_prefix("git://") {
        Cow::Owned(format!("https://{rest}"))
    } else {
        Cow::Borrowed(input)
    };

    let mut parsed = url::Url::parse(&cleaned).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }

    let fragment = try_extract_fragment(raw_url);
    parsed.set_fragment(None);
    parsed.set_query(None);

    let mut url = parsed.to_string();
    if url.ends_with('/') {
        url.pop();
    }
    if url.ends_with(".git") {
        url.truncate(url.len() - 4);
    }

    let base_url = url;

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or("HEAD");
        format!("{base_url}/tree/{branch}/{}", dir.trim_start_matches('/'))
    } else if let Some(branch) = fragment {
        format!("{base_url}/tree/{branch}")
    } else {
        base_url
    })
}

struct HostedRepo {
    base_url: String,
    default_branch: &'static str,
}

fn try_hosted_shorthand(raw_url: &str, directory: Option<&str>) -> Option<String> {
    let cleaned =
        raw_url.strip_prefix("git+").unwrap_or(raw_url).strip_prefix("git://").unwrap_or(raw_url);

    let (hosted, path) = if let Some(rest) = cleaned.strip_prefix("github:") {
        (HostedRepo { base_url: "https://github.com".to_string(), default_branch: "master" }, rest)
    } else if let Some(rest) = cleaned.strip_prefix("gitlab:") {
        (HostedRepo { base_url: "https://gitlab.com".to_string(), default_branch: "master" }, rest)
    } else if let Some(rest) = cleaned.strip_prefix("bitbucket:") {
        (
            HostedRepo { base_url: "https://bitbucket.org".to_string(), default_branch: "master" },
            rest,
        )
    } else {
        return try_user_repo_shorthand(raw_url, directory);
    };

    let fragment = try_extract_fragment(raw_url);
    let path_clean = path.split(&['#', '?'][..]).next().unwrap_or(path).trim_end_matches('/');
    let path_no_git = path_clean.trim_end_matches(".git");
    let parts: Vec<&str> = path_no_git.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let hosted_base_url = &hosted.base_url;
    let browse_path = format!("{hosted_base_url}/{}", parts[..2].join("/"));

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or(hosted.default_branch);
        format!("{browse_path}/tree/{branch}/{}", dir.trim_start_matches('/'))
    } else if let Some(branch) = fragment {
        format!("{browse_path}/tree/{branch}")
    } else {
        browse_path
    })
}

fn try_user_repo_shorthand(raw_url: &str, directory: Option<&str>) -> Option<String> {
    let cleaned = raw_url.strip_prefix("git+").unwrap_or(raw_url);

    if cleaned.contains("://") || cleaned.starts_with("git@") {
        return try_hosted_url(raw_url, directory);
    }

    if let Some(rest) = cleaned.strip_prefix("github:") {
        return build_hosted_browse_url("https://github.com", rest, "master", directory);
    }

    if let Some(rest) = cleaned.strip_prefix("gitlab:") {
        return build_hosted_browse_url("https://gitlab.com", rest, "master", directory);
    }

    if let Some(rest) = cleaned.strip_prefix("bitbucket:") {
        return build_hosted_browse_url("https://bitbucket.org", rest, "master", directory);
    }

    let fragment = try_extract_fragment(raw_url);
    let path_clean = cleaned.split(&['#', '?'][..]).next().unwrap_or(cleaned).trim_end_matches('/');

    if !path_clean.contains('/') {
        return None;
    }

    let parts: Vec<&str> = path_clean.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let user = parts[0];
    let repo = parts[1].trim_end_matches(".git");

    if user.contains('.') {
        return try_hosted_url(raw_url, directory);
    }

    let browse_path = format!("https://github.com/{user}/{repo}");

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or("master");
        format!("{browse_path}/tree/{branch}/{}", dir.trim_start_matches('/'))
    } else if let Some(branch) = fragment {
        format!("{browse_path}/tree/{branch}")
    } else {
        browse_path
    })
}

fn try_hosted_url(raw_url: &str, directory: Option<&str>) -> Option<String> {
    let input = raw_url.strip_prefix("git+").unwrap_or(raw_url);

    let (parsed, fragment) = if let Some(rest) = input.strip_prefix("git@") {
        // SCP-style SSH: git@<host>:<owner>/<repo>(.git)?(#branch)?
        let (scp_host, scp_path) = rest.split_once(':')?;
        let path_only = scp_path.split(&['#', '?'][..]).next().unwrap_or(scp_path);
        let parsed = url::Url::parse(&format!("https://{scp_host}/{path_only}")).ok()?;
        let frag = try_extract_fragment(raw_url);
        (parsed, frag)
    } else {
        let normalized = if let Some(rest) = input.strip_prefix("git://") {
            Cow::Owned(format!("https://{rest}"))
        } else {
            Cow::Borrowed(input)
        };
        let frag = try_extract_fragment(raw_url);
        let parsed = url::Url::parse(&normalized).ok()?;
        (parsed, frag)
    };

    let host = parsed.host_str()?;

    let (base_url, default_branch) = match host {
        "github.com" => ("https://github.com", "master"),
        "gitlab.com" => ("https://gitlab.com", "master"),
        "bitbucket.org" | "bitbucket.com" => ("https://bitbucket.org", "master"),
        _ => return None,
    };

    let path_clean = parsed.path().trim_end_matches('/');
    let repo_path = path_clean.strip_prefix('/').unwrap_or(path_clean).trim_end_matches(".git");

    let browse_path = format!("{base_url}/{repo_path}");

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or(default_branch);
        format!("{browse_path}/tree/{branch}/{}", dir.trim_start_matches('/'))
    } else if let Some(branch) = fragment {
        format!("{browse_path}/tree/{branch}")
    } else {
        browse_path
    })
}

fn build_hosted_browse_url(
    base_url: &str,
    path: &str,
    default_branch: &str,
    directory: Option<&str>,
) -> Option<String> {
    let path_clean = path.split(&['#', '?'][..]).next().unwrap_or(path).trim_end_matches('/');
    let path_no_git = path_clean.trim_end_matches(".git");
    let parts: Vec<&str> = path_no_git.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let browse_path = format!("{base_url}/{}", parts[..2].join("/"));
    let fragment = try_extract_fragment(path);

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or(default_branch);
        format!("{browse_path}/tree/{branch}/{}", dir.trim_start_matches('/'))
    } else if let Some(branch) = fragment {
        format!("{browse_path}/tree/{branch}")
    } else {
        browse_path
    })
}

fn try_extract_fragment(raw_url: &str) -> Option<String> {
    let (_, after_hash) = raw_url.split_once('#')?;
    let fragment = after_hash.split('?').next()?;
    if fragment.is_empty() { None } else { Some(fragment.to_string()) }
}

fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let status = std::process::Command::new("xdg-open").arg(url).status()?;
        if status.success() { Ok(()) } else { Err(std::io::Error::other("xdg-open failed")) }
    }
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open").arg(url).status()?;
        if status.success() { Ok(()) } else { Err(std::io::Error::other("open failed")) }
    }
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let url_wide: Vec<u16> = OsStr::new(url).encode_wide().chain(std::iter::once(0)).collect();

        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                url_wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            )
        };
        if (result as isize) > 32 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported platform"))
    }
}

fn redact_url(url: &str) -> String {
    url::Url::parse(url).map_or_else(
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
