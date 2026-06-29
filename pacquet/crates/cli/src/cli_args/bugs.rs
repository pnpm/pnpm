use crate::cli_args::registry_client::build_registry_client;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::encode_package_name;
use pacquet_package_manifest::safe_read_package_json_from_dir;
use pacquet_registry::{PackageTag, PackageVersion};
use serde_json::Value;
use std::path::Path;
use url::Url;

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
            let registry_url =
                normalize_registry_url(self.registry.as_deref().unwrap_or(&config.registry));
            let http_client = build_registry_client(config)
                .wrap_err("build the network client for registry requests")?;

            for spec in &self.packages {
                let url = get_bugs_url_from_registry(
                    spec,
                    &registry_url,
                    &http_client,
                    &config.auth_headers,
                )
                .await
                .wrap_err_with(|| format!("look up bugs URL for \"{spec}\""))?;
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
    let (package_name, _tag) = parse_package_spec(spec);
    let encoded_name = encode_package_name(package_name);
    let package_version = PackageVersion::fetch_from_registry(
        &encoded_name,
        PackageTag::Latest,
        http_client,
        registry_url,
        auth_headers,
    )
    .await
    .into_diagnostic()
    .wrap_err_with(|| format!("fetch package info for \"{package_name}\" from the registry"))?;

    let manifest = package_manifest_from_version(&package_version);
    pick_bugs_url(&manifest)
        .ok_or_else(|| BugsError::NoBugsUrlForPackage { package: package_name.to_string() }.into())
}

fn package_manifest_from_version(version: &PackageVersion) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("name".to_string(), Value::String(version.name.clone()));
    for (key, value) in &version.other {
        map.insert(key.clone(), value.clone());
    }
    Value::Object(map)
}

fn pick_bugs_url(manifest: &Value) -> Option<String> {
    if let Some(bugs) = manifest.get("bugs") {
        let url = match bugs {
            Value::String(s) => Some(s.clone()),
            Value::Object(m) => m.get("url").and_then(|v| v.as_str()).map(String::from),
            _ => None,
        };
        if url.as_ref().is_some_and(|u| is_http_url(u)) {
            return url;
        }
    }

    if let Some(repo) = manifest.get("repository") {
        let url = match repo {
            Value::String(s) => Some(s.clone()),
            Value::Object(m) => m.get("url").and_then(|v| v.as_str()).map(String::from),
            _ => None,
        };
        if let Some(ref url) = url {
            return repository_to_issues_url(url);
        }
    }

    None
}

fn repository_to_issues_url(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();

    if let Some(url) = try_hosted_git_shorthand(trimmed) {
        return Some(url);
    }

    let cleaned = trimmed.strip_prefix("git+").unwrap_or(trimmed);

    if let Some(rest) = cleaned.strip_prefix("git@")
        && let Some(colon_pos) = rest.find(':')
    {
        let host = &rest[..colon_pos];
        let path = rest[colon_pos + 1..].trim_end_matches(".git").trim_end_matches('/');
        if !host.is_empty() && !path.is_empty() {
            return Some(format!("https://{host}/{path}/issues"));
        }
    }

    let parsed = Url::parse(cleaned).ok()?;

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
            Some(format!("https://{host}{path}/issues"))
        }
        _ => None,
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
        return Some(format!("https://github.com/{user}/{project}/issues"));
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
    println!("{url}");
    let result = open_url_in_browser(url);
    if let Err(err) = result {
        tracing::debug!(target: "pacquet_cli", %err, "could not open browser");
    }
}

#[cfg(target_os = "linux")]
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let rundll32 = std::path::Path::new(&system_root).join("System32").join("rundll32.exe");
    std::process::Command::new(rundll32)
        .args(["url.dll,FileProtocolHandler", url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
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
