//! `pacquet search` — search for packages in the registry.

use crate::cli_args::{registry_client::build_registry_client, sanitize::sanitize};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic, WrapErr};
use owo_colors::{OwoColorize, Stream};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, redact_and_sanitize, send_with_retry};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum SearchError {
    #[display("Search query is required. Usage: pnpm search <keyword>")]
    #[diagnostic(code(ERR_PNPM_MISSING_SEARCH_QUERY))]
    MissingQuery,

    #[display("Search failed with status {status}: {status_text}{detail}")]
    #[diagnostic(code(ERR_PNPM_SEARCH_FAILED))]
    SearchFailed { status: u16, status_text: String, detail: String },

    #[display("Network request failed: {message}")]
    #[diagnostic(code(ERR_PNPM_SEARCH_FAILED))]
    NetworkError { message: String },
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Show search results in JSON format.
    #[clap(long)]
    pub json: bool,

    /// Maximum number of results to show (default: 20).
    #[clap(long = "search-limit")]
    pub search_limit: Option<u32>,

    /// Registry URL to search in.
    #[clap(long)]
    pub registry: Option<String>,

    /// Search query terms.
    pub query: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchAuthor {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AuthorInfo {
    Object(SearchAuthor),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPublisher {
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMaintainer {
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPackage {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub date: Option<String>,
    pub author: Option<AuthorInfo>,
    pub publisher: Option<SearchPublisher>,
    pub maintainers: Option<Vec<SearchMaintainer>>,
    pub keywords: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawSearchResult {
    pub package: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistrySearchResponse {
    pub objects: Vec<RawSearchResult>,
}

impl SearchArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<String> {
        let query_string = self.query.join(" ");
        if query_string.is_empty() {
            return Err(SearchError::MissingQuery.into());
        }

        let registry_url = self.registry.as_deref().unwrap_or(&config.registry);
        // Add a trailing slash before joining so a registry with a path
        // prefix keeps it.
        let normalized_registry_url = if registry_url.ends_with('/') {
            registry_url.to_owned()
        } else {
            format!("{registry_url}/")
        };

        let base_url = url::Url::parse(&normalized_registry_url)
            .map_err(|err| SearchError::NetworkError { message: err.to_string() })?;
        let mut search_url = base_url
            .join("./-/v1/search")
            .map_err(|err| SearchError::NetworkError { message: err.to_string() })?;

        search_url
            .query_pairs_mut()
            .append_pair("text", &query_string)
            .append_pair("size", &self.search_limit.unwrap_or(20).to_string());

        let auth_header = config.auth_headers.for_url(&normalized_registry_url);
        let http_client = build_registry_client(config)?;

        let retry_opts = RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        };

        let (client, response) =
            send_with_retry(&http_client, search_url.as_str(), retry_opts, |client| {
                let mut request = client.get(search_url.as_str());
                if let Some(ref header) = auth_header {
                    request = request.header("authorization", header.as_str());
                }
                request
            })
            .await
            .map_err(|error| SearchError::NetworkError {
                message: redact_and_sanitize(&error.to_string()),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default().trim().to_string();
            let detail =
                if error_body.is_empty() { String::new() } else { format!(". {error_body}") };
            return Err(SearchError::SearchFailed {
                status: status.as_u16(),
                status_text: status.canonical_reason().unwrap_or_default().to_string(),
                detail,
            }
            .into());
        }

        let data = response
            .json::<RegistrySearchResponse>()
            .await
            .into_diagnostic()
            .wrap_err("parsing the search response")?;

        drop(client);

        if self.json {
            let packages: Vec<&serde_json::Value> =
                data.objects.iter().map(|obj| &obj.package).collect();
            return Ok(serde_json::to_string_pretty(&packages)
                .map_err(|err| SearchError::NetworkError { message: err.to_string() })?);
        }

        if data.objects.is_empty() {
            return Ok("No packages found".to_string());
        }

        let mut formatted_packages = Vec::new();
        for obj in data.objects {
            let pkg: SearchPackage = serde_json::from_value(obj.package)
                .map_err(|err| SearchError::NetworkError { message: err.to_string() })?;
            formatted_packages.push(format_package(&pkg));
        }

        Ok(formatted_packages.join("\n\n"))
    }
}

fn format_package(pkg: &SearchPackage) -> String {
    let author = if let Some(ref author_info) = pkg.author {
        match author_info {
            AuthorInfo::Object(author_obj) => author_obj.name.clone(),
            AuthorInfo::String(author_str) => author_str.clone(),
        }
    } else if let Some(ref publisher) = pkg.publisher {
        publisher.username.clone()
    } else {
        String::new()
    };

    let date = if let Some(ref date_str) = pkg.date {
        date_str.split('T').next().unwrap_or("").to_owned()
    } else {
        String::new()
    };

    let mut lines = Vec::new();
    lines.push(bold(&pkg.name));

    if let Some(ref desc) = pkg.description {
        lines.push(sanitize(desc).into_owned());
    }

    let mut version_line = vec![format!("Version {}", pkg.version)];
    if !date.is_empty() {
        version_line.push(format!("published {date}"));
    }
    if !author.is_empty() {
        version_line.push(format!("by {author}"));
    }
    lines.push(version_line.join(" "));

    if let Some(ref maintainers) = pkg.maintainers
        && !maintainers.is_empty()
    {
        let usernames: Vec<String> =
            maintainers.iter().map(|maintainer| maintainer.username.clone()).collect();
        lines.push(format!("Maintainers: {}", usernames.join(", ")));
    }

    if let Some(ref keywords) = pkg.keywords
        && !keywords.is_empty()
    {
        lines.push(format!("Keywords: {}", keywords.join(", ")));
    }

    lines.push(bright_blue(&format!("https://npmx.dev/package/{}", pkg.name)));

    lines.join("\n")
}

fn bold(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn bright_blue(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bright_blue()).to_string()
}
