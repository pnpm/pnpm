use std::path::Path;

use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pacquet_config::Config;
use pacquet_package_manifest::safe_read_package_json_from_dir;
use pacquet_registry::{PackageTag, PackageVersion};
use serde_json::Value;
use url::Url;

use crate::cli_args::registry_client::build_registry_client;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum BugsError {
    #[display(
        "The current project does not have a bug tracker URL. \
         Add a \"bugs\" or \"repository\" field to its manifest."
    )]
    #[diagnostic(code(ERR_PNPM_NO_BUGS_URL))]
    NoBugsUrl,

    #[display("The package \"{package}\" does not have a bug tracker URL.")]
    #[diagnostic(code(ERR_PNPM_NO_BUGS_URL))]
    NoBugsUrlForPackage { package: String },

    #[display("Registry request to {url} failed: {reason}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryError { url: String, reason: String },
}

#[derive(Debug, clap::Args)]
pub struct BugsArgs {
    #[clap(long)]
    pub registry: Option<String>,

    pub packages: Vec<String>,
}

impl BugsArgs {
    pub async fn run(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        if self.packages.is_empty() {
            let url = get_bugs_url_from_current_project(dir)?;
            open_url(&url);
        } else {
            let http_client = build_registry_client(config)
                .wrap_err("build the network client for registry requests")?;

            let registries: std::collections::HashMap<String, String> =
                config.resolved_registries().into_iter().collect();

            let futures = self.packages.iter().map(|spec| {
                let (package_name, _tag) = parse_package_spec(spec);
                let target_registry = if let Some(ref override_registry) = self.registry {
                    normalize_registry_url(override_registry)
                } else {
                    let picked = pacquet_resolving_npm_resolver::pick_registry_for_package(
                        &registries,
                        package_name,
                        Some(spec),
                    );
                    normalize_registry_url(&picked)
                };

                let http_client = &http_client;
                let auth_headers = &config.auth_headers;
                async move {
                    get_bugs_url_from_registry(spec, &target_registry, http_client, auth_headers)
                        .await
                        .wrap_err_with(|| format!("look up bugs URL for \"{spec}\""))
                }
            });

            let results: Vec<miette::Result<String>> =
                futures_util::future::join_all(futures).await;
            for res in results {
                let url = res?;
                open_url(&url);
            }
        }
        Ok(())
    }
}

fn get_bugs_url_from_current_project(dir: &Path) -> miette::Result<String> {
    let manifest =
        safe_read_package_json_from_dir(dir).wrap_err("read package.json")?.ok_or_else(|| {
            let display_path = dir.display();
            miette::miette!(
                code = "ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND",
                "No package.json was found in {display_path}",
            )
        })?;

    pick_bugs_url(&manifest).ok_or_else(|| BugsError::NoBugsUrl.into())
}

async fn get_bugs_url_from_registry(
    spec: &str,
    registry_url: &str,
    http_client: &pacquet_network::ThrottledClient,
    auth_headers: &pacquet_network::AuthHeaders,
) -> miette::Result<String> {
    let (package_name, tag) = parse_package_spec(spec);
    let package_tag = match tag {
        None => PackageTag::Latest,
        Some(tag_str) => tag_str.parse::<PackageTag>().unwrap_or(PackageTag::Latest),
    };
    let package_version = PackageVersion::fetch_from_registry(
        package_name,
        package_tag,
        http_client,
        registry_url,
        auth_headers,
    )
    .await
    .map_err(|err| {
        let (url, reason) = match err {
            pacquet_registry::RegistryError::Network(net_err) => (
                pacquet_network::redact_url_credentials(&net_err.url),
                pacquet_network::redact_url_credentials(&net_err.error.to_string()),
            ),
            other => (
                pacquet_network::redact_url_credentials(registry_url),
                pacquet_network::redact_url_credentials(&other.to_string()),
            ),
        };
        BugsError::RegistryError { url, reason }
    })
    .wrap_err_with(|| format!("fetch package info for \"{package_name}\" from the registry"))?;

    let manifest = package_manifest_from_version(&package_version);
    pick_bugs_url(&manifest)
        .ok_or_else(|| BugsError::NoBugsUrlForPackage { package: package_name.to_string() }.into())
}

fn package_manifest_from_version(version: &PackageVersion) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("name".to_string(), Value::String(version.name.clone()));
    if let Some(bugs) = version.other.get("bugs") {
        map.insert("bugs".to_string(), bugs.clone());
    }
    if let Some(repo) = version.other.get("repository") {
        map.insert("repository".to_string(), repo.clone());
    }
    Value::Object(map)
}

fn pick_bugs_url(manifest: &Value) -> Option<String> {
    if let Some(bugs) = manifest.get("bugs") {
        let url = match bugs {
            Value::String(url_str) => Some(url_str.clone()),
            Value::Object(bugs_obj) => {
                bugs_obj.get("url").and_then(|val| val.as_str()).map(String::from)
            }
            _ => None,
        };
        if url.as_ref().is_some_and(|url_str| is_http_url(url_str)) {
            return url;
        }
    }

    if let Some(repo) = manifest.get("repository") {
        let url = match repo {
            Value::String(url_str) => Some(url_str.clone()),
            Value::Object(repo_obj) => {
                repo_obj.get("url").and_then(|val| val.as_str()).map(String::from)
            }
            _ => None,
        };
        if let Some(ref url) = url {
            return repository_to_issues_url(url);
        }
    }

    None
}

fn repository_to_issues_url(raw_url: &str) -> Option<String> {
    let mut trimmed = raw_url.trim();

    // Strip fragment and query first to prevent them from leaking into shorthand or SCP paths
    if let Some(pos) = trimmed.find('#') {
        trimmed = &trimmed[..pos];
    }
    if let Some(pos) = trimmed.find('?') {
        trimmed = &trimmed[..pos];
    }

    if let Some(url) = try_hosted_git_shorthand(trimmed) {
        return Some(url);
    }

    let cleaned = trimmed.strip_prefix("git+").unwrap_or(trimmed);

    // Handle SCP-style SSH URLs: `git@github.com:owner/repo.git`
    if let Some(rest) = cleaned.strip_prefix("git@")
        && let Some(colon_pos) = rest.find(':')
    {
        let host = &rest[..colon_pos];
        let path = rest[colon_pos + 1..].trim_end_matches(".git").trim_end_matches('/');
        if !host.is_empty() && !path.is_empty() {
            return Some(format!("https://{host}/{path}/issues"));
        }
    }

    let parsed_url = if let Ok(parsed) = Url::parse(cleaned) {
        Some(parsed)
    } else if cleaned.contains('/') && !cleaned.contains(':') {
        let slash_pos = cleaned.find('/');
        let dot_pos = cleaned.find('.');
        if let Some(slash_pos) = slash_pos
            && let Some(dot_pos) = dot_pos
            && dot_pos < slash_pos
        {
            Url::parse(&format!("https://{cleaned}")).ok()
        } else {
            None
        }
    } else {
        None
    };

    if let Some(parsed) = parsed_url {
        match parsed.scheme() {
            "http" | "https" => {
                let mut url = parsed;
                url.set_query(None);
                url.set_fragment(None);
                let path = url.path().trim_end_matches('/').trim_end_matches(".git");
                if path.is_empty() {
                    return None;
                }
                let new_path = format!("{path}/issues");
                url.set_path(&new_path);
                Some(url.to_string())
            }
            "ssh" | "git" | "git+ssh" => {
                let host = parsed.host_str()?;
                let path = parsed.path().trim_end_matches('/').trim_end_matches(".git");
                if path.is_empty() {
                    return None;
                }
                if let Some(port) = parsed.port() {
                    Some(format!("https://{host}:{port}{path}/issues"))
                } else {
                    Some(format!("https://{host}{path}/issues"))
                }
            }
            _ => None,
        }
    } else {
        None
    }
}

fn try_hosted_git_shorthand(input: &str) -> Option<String> {
    if let Some(rest) = input.strip_prefix("github:") {
        let (user, project) = split_user_project(rest)?;
        return Some(format!("https://github.com/{user}/{project}/issues"));
    }
    if let Some(rest) = input.strip_prefix("gitlab:") {
        let (user, project) = split_user_project(rest)?;
        return Some(format!("https://gitlab.com/{user}/{project}/issues"));
    }
    if let Some(rest) = input.strip_prefix("bitbucket:") {
        let (user, project) = split_user_project(rest)?;
        return Some(format!("https://bitbucket.org/{user}/{project}/issues"));
    }

    // `owner/repo` shorthand — only when no `:`, `//`, or `@` is present,
    // to avoid matching `git+<https://`>, SCP-style SSH, etc.
    if !input.contains(':') && !input.contains("//") && !input.contains('@') {
        let (user, project) = split_user_project(input)?;
        if !project.contains('/') {
            return Some(format!("https://github.com/{user}/{project}/issues"));
        }
    }

    None
}

fn split_user_project(spec: &str) -> Option<(&str, &str)> {
    let spec = spec.trim_end_matches(".git");
    let slash_pos = spec.find('/')?;
    let user = &spec[..slash_pos];
    let project = &spec[slash_pos + 1..];
    if user.is_empty() || project.is_empty() {
        return None;
    }
    Some((user, project))
}

fn is_http_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|parsed| parsed.scheme() == "http" || parsed.scheme() == "https")
}

fn normalize_registry_url(url: &str) -> String {
    if url.ends_with('/') { url.to_owned() } else { format!("{url}/") }
}

fn open_url(url: &str) {
    let sanitized = crate::cli_args::sanitize::sanitize(url);
    let redacted = pacquet_network::redact_url_credentials(&sanitized);
    println!("{redacted}");

    // Clear username/password before passing to open_url_in_browser:
    let clean_url_for_browser = if let Ok(mut parsed) = Url::parse(url) {
        if !parsed.username().is_empty() || parsed.password().is_some() {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    };

    let clean_url_for_browser = crate::cli_args::sanitize::sanitize(&clean_url_for_browser);

    let result = open_url_in_browser(&clean_url_for_browser);
    if let Err(err) = result {
        tracing::debug!(target: "pacquet_cli", %err, "could not open browser");
    }
}

#[cfg(target_os = "linux")]
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let url_wide: Vec<u16> =
        OsStr::new(url).encode_wide().chain(std::iter::once(0)).collect();

    // ShellExecuteW invokes the default handler for the URL protocol
    // without shell metacharacter injection. The function takes a
    // fully-qualified path (or registered protocol) so it does not
    // depend on the executable search path or the SystemRoot env var,
    // avoiding the hijack vector that rundll32.exe is subject to.
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(), // hwnd
            std::ptr::null(),     // lpOperation (null => "open")
            url_wide.as_ptr(),    // lpFile
            std::ptr::null(),     // lpParameters
            std::ptr::null(),     // lpDirectory
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if (result as isize) > 32 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn open_url_in_browser(_url: &str) -> std::io::Result<()> {
    Ok(())
}

fn parse_package_spec(spec: &str) -> (&str, Option<&str>) {
    let spec = spec.trim();
    if let Some(stripped) = spec.strip_prefix('@') {
        if let Some(at_pos) = stripped.rfind('@')
            && at_pos > 0
        {
            let split_pos = at_pos + 1;
            return (&spec[..split_pos], Some(&spec[split_pos + 1..]));
        }
        (spec, None)
    } else if let Some(at_pos) = spec.rfind('@')
        && at_pos > 0
    {
        (&spec[..at_pos], Some(&spec[at_pos + 1..]))
    } else {
        (spec, None)
    }
}

#[cfg(test)]
mod tests;
