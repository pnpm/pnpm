//! `pacquet view` (aliases `info`, `show`, `v`) — print package metadata
//! from the registry.
//!
//! When the package name is omitted, the nearest project manifest's `name`
//! is used (searching upward from `--dir`). With one or more trailing
//! field arguments, only those fields are printed; otherwise a formatted
//! summary (or, with `--json`, the whole assembled info object) is shown.

use std::path::Path;

use chrono::{DateTime, Utc};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use owo_colors::{OwoColorize, Stream, Style};
use pacquet_config::Config;
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, PickPackageFromMetaOptions,
    fetch_full_metadata, parse_bare_specifier, pick_package_from_meta, pick_registry_for_package,
    pick_version_by_version_range,
};
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pacquet_workspace::try_read_project_manifest;
use serde_json::{Map, Value};

use super::deprecate::normalize_registry_url;

/// Errors from `pacquet view`. The codes are the `ERR_PNPM_*` codes pnpm
/// defines for these failures.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ViewError {
    #[display("Package name is required. Usage: pnpm view [<package-name>]")]
    #[diagnostic(code(ERR_PNPM_MISSING_PACKAGE_NAME))]
    MissingPackageName,

    #[display("Invalid package name: \"{spec}\". This command only supports registry packages.")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        spec: String,
    },

    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_JSON))]
    InvalidPackageJson {
        #[error(not(source))]
        message: String,
    },

    #[display("No matching version found for {name}@{spec}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound {
        #[error(not(source))]
        name: String,
        spec: String,
    },

    #[display("GET {url}: Not Found - 404")]
    #[diagnostic(code(ERR_PNPM_FETCH_404))]
    Fetch404 {
        #[error(not(source))]
        url: String,
    },
}

#[derive(Debug, Args)]
pub struct ViewArgs {
    /// `<package-name>[@<version>]` followed by optional fields to print
    /// (e.g. `view foo dist.tarball version`). When the package name is
    /// omitted, the nearest project manifest's name is used.
    pub params: Vec<String>,

    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// Show information in JSON format.
    #[clap(long)]
    pub json: bool,
}

impl ViewArgs {
    /// Resolve the package spec (the first positional, or the nearest
    /// manifest's name when omitted), fetch its registry metadata, pick the
    /// matching version, and render the requested fields, a JSON dump, or the
    /// formatted summary.
    pub async fn run(self, config: &Config, dir: &Path) -> miette::Result<String> {
        let package_spec = match self.params.first() {
            Some(spec) => spec.clone(),
            None => nearest_manifest_name(dir)?,
        };
        let fields = self.params.get(1..).unwrap_or(&[]);

        let info = fetch_package_info(config, self.registry.as_deref(), &package_spec).await?;

        if !fields.is_empty() {
            return Ok(render_fields(&info, fields, self.json));
        }
        if self.json {
            return Ok(to_pretty(&info));
        }
        Ok(render_summary(&info))
    }
}

/// Find the nearest project manifest's `name`, searching upward from
/// `start_dir`. A missing manifest is [`ViewError::MissingPackageName`]; a
/// present-but-invalid manifest (parse error, non-object body, or no `name`)
/// is [`ViewError::InvalidPackageJson`].
fn nearest_manifest_name(start_dir: &Path) -> Result<String, ViewError> {
    let mut dir = start_dir;
    loop {
        let manifest_path = dir.join("package.json");
        if manifest_path.is_file() {
            let manifest =
                try_read_project_manifest(dir).map_err(|err| ViewError::InvalidPackageJson {
                    message: format!(
                        r#"Failed to read or parse project manifest in "{dir}": {err}"#,
                        dir = dir.display(),
                    ),
                })?;
            let value = manifest.map_or(Value::Null, |(_, manifest)| manifest.value().clone());
            if !value.is_object() {
                return Err(invalid_manifest(dir));
            }
            return match value.get("name").and_then(Value::as_str).filter(|name| !name.is_empty()) {
                Some(name) => Ok(name.to_string()),
                None => Err(invalid_manifest(dir)),
            };
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Err(ViewError::MissingPackageName),
        }
    }
}

/// The `ERR_PNPM_INVALID_PACKAGE_JSON` raised when a found manifest is not a
/// usable object or lacks a non-empty `name`.
fn invalid_manifest(dir: &Path) -> ViewError {
    ViewError::InvalidPackageJson {
        message: format!(
            r#"Invalid package.json at "{}". The "name" field is required and must be a non-empty string."#,
            dir.display(),
        ),
    }
}

/// Fetch and assemble the registry info object for `package_spec`: parse the
/// spec (rejecting non-registry protocols), fetch full metadata, pick the
/// version satisfying the spec, then extend that version's manifest with the
/// packument-level `versions`, `dist-tags`, and `time` fields the renderers
/// read.
async fn fetch_package_info(
    config: &Config,
    registry_override: Option<&str>,
    package_spec: &str,
) -> miette::Result<Value> {
    let parsed = parse_wanted_dependency(package_spec);
    let alias = parsed.alias.as_deref();
    let bare = parsed.bare_specifier.as_deref().unwrap_or("latest");
    let name_hint = alias.unwrap_or(package_spec);

    let mut registries: std::collections::HashMap<String, String> =
        config.resolved_registries().into_iter().collect();
    if let Some(registry) = registry_override {
        registries.insert("default".to_string(), normalize_registry_url(registry));
    }
    let registry = pick_registry_for_package(&registries, name_hint, Some(bare));

    let spec = parse_bare_specifier(bare, alias, "latest", &registry)
        .ok_or_else(|| ViewError::InvalidPackageName { spec: package_spec.to_string() })?;

    let http_client = ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: std::time::Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
    )
    .into_diagnostic()
    .wrap_err("create the network client for view")?;

    let outcome = fetch_full_metadata(
        &spec.name,
        &FetchFullMetadataOptions {
            registry: &registry,
            http_client: &http_client,
            auth_headers: &config.auth_headers,
            full_metadata: true,
            etag: None,
            modified: None,
            retry_opts: RetryOpts::default(),
        },
    )
    .await
    .map_err(|error| map_fetch_error(error, &registry, &spec.name))?;

    let meta = match outcome {
        FetchFullMetadataOutcome::Modified(meta) => *meta,
        FetchFullMetadataOutcome::NotModified => {
            miette::bail!("registry returned 304 Not Modified unexpectedly for {}", spec.name)
        }
    };

    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &meta,
        &spec,
    )?
    .package
    .ok_or_else(|| ViewError::PackageNotFound {
        name: spec.name.clone(),
        spec: spec.fetch_spec.clone(),
    })?;

    Ok(assemble_info(&meta, &picked))
}

/// Build the info object the renderers consume from the picked version's
/// manifest and the packument-level fields. The picked version's raw JSON
/// fragment is reused so registry key order is preserved in `--json` output,
/// and the extra fields are appended in place.
fn assemble_info(
    meta: &pacquet_registry::Package,
    picked: &pacquet_registry::PackageVersion,
) -> Value {
    let version_key = picked.version.to_string();
    let data = meta
        .versions
        .fragments()
        .find(|(version, _)| version.as_str() == version_key)
        .and_then(|(_, json)| serde_json::from_str::<Value>(&json).ok())
        .unwrap_or_else(|| serde_json::to_value(picked).unwrap_or(Value::Null));

    let mut info = match data {
        Value::Object(map) => map,
        _ => Map::new(),
    };

    // An object author collapses to its `name` (dropped entirely when it has
    // none); a string author is left untouched.
    if let Some(Value::Object(author)) = info.get("author") {
        match author.get("name").cloned() {
            Some(name) => {
                info.insert("author".to_string(), name);
            }
            None => {
                info.shift_remove("author");
            }
        }
    }

    let versions: Vec<&String> = meta.versions.keys().collect();
    let versions_count = versions.len();
    let deps_count = picked.dependencies.as_ref().map_or(0, std::collections::HashMap::len);

    info.insert("versions".to_string(), serde_json::to_value(&versions).unwrap_or(Value::Null));
    if versions_count > 0 {
        info.insert("versionsCount".to_string(), serde_json::json!(versions_count));
    }
    if deps_count > 0 {
        info.insert("depsCount".to_string(), serde_json::json!(deps_count));
    }
    let dist_tags = serde_json::to_value(&meta.dist_tags).unwrap_or(Value::Null);
    info.insert("distTags".to_string(), dist_tags.clone());
    info.insert("dist-tags".to_string(), dist_tags);
    if let Some(time) = &meta.time {
        info.insert("time".to_string(), serde_json::to_value(time).unwrap_or(Value::Null));
    }

    Value::Object(info)
}

/// Map a metadata-fetch failure to the matching pnpm error. A `404`
/// becomes [`ViewError::Fetch404`] (pnpm's `ERR_PNPM_FETCH_404`); every
/// other failure is surfaced verbatim.
fn map_fetch_error(
    error: pacquet_resolving_npm_resolver::FetchMetadataError,
    registry: &str,
    pkg_name: &str,
) -> miette::Report {
    use pacquet_resolving_npm_resolver::FetchMetadataError;
    if let FetchMetadataError::Network { error: ref source, .. } = error
        && source.status() == Some(reqwest::StatusCode::NOT_FOUND)
    {
        return ViewError::Fetch404 {
            url: pacquet_network::redact_url_credentials(&format!("{registry}{pkg_name}")),
        }
        .into();
    }
    miette::Report::new(error)
}

/// Render the selected `fields` of `info`. A single field unwraps to its
/// value (raw for `--json`, plain for text); multiple fields render as a
/// `{field: value}` object (`--json`) or `field = value` lines.
fn render_fields(info: &Value, fields: &[String], json: bool) -> String {
    let selected: Vec<(&String, Option<Value>)> =
        fields.iter().map(|field| (field, get_nested_property(info, field))).collect();

    if json {
        if let [(_, value)] = selected.as_slice() {
            return value.as_ref().map(to_pretty).unwrap_or_default();
        }
        let map: Map<String, Value> = selected
            .iter()
            .filter_map(|(field, value)| value.clone().map(|value| ((*field).clone(), value)))
            .collect();
        return to_pretty(&Value::Object(map));
    }

    if let [(_, value)] = selected.as_slice() {
        return format_field_value(value.as_ref());
    }

    selected
        .iter()
        .map(|(field, value)| match value {
            Some(value @ (Value::Object(_) | Value::Array(_))) => {
                format!("{field} = {}", serde_json::to_string(value).unwrap_or_default())
            }
            Some(Value::String(string)) => format!("{field} = '{string}'"),
            other => format!("{field} = {}", format_field_value(other.as_ref())),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Walk a dotted `path` (e.g. `dist.shasum`) through `info`, returning a
/// clone of the value or `None` when any segment is missing or not an
/// object.
fn get_nested_property(info: &Value, path: &str) -> Option<Value> {
    let mut current = info;
    for part in path.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current.clone())
}

/// Stringify a single field value for text output: `null`/absent is empty,
/// objects and arrays are pretty JSON, strings pass through, and scalars use
/// their plain form.
fn format_field_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(value @ (Value::Object(_) | Value::Array(_))) => to_pretty(value),
        Some(Value::String(string)) => string.clone(),
        Some(other) => other.to_string(),
    }
}

/// Render the human-readable summary block: a `name@version | license |
/// deps | versions` header followed by description, homepage, deprecation,
/// keywords, bin, dist, dependencies, maintainers, dist-tags, and the
/// published-by line.
fn render_summary(info: &Value) -> String {
    let mut header: Vec<String> = Vec::new();
    if let (Some(name), Some(version)) = (str_field(info, "name"), str_field(info, "version")) {
        header.push(cyan(&format!("{name}@{version}")));
    }
    if let Some(license) = str_field(info, "license") {
        header.push(green(license));
    }
    match info.get("depsCount").and_then(Value::as_u64) {
        Some(count) => header.push(format!("deps: {}", cyan(&count.to_string()))),
        None => header.push("deps: none".to_string()),
    }
    if let Some(count) = info.get("versionsCount").and_then(Value::as_u64) {
        header.push(format!("versions: {}", cyan(&count.to_string())));
    }

    let mut lines: Vec<String> = vec![header.join(" | ")];

    if let Some(description) = str_field(info, "description") {
        lines.push(description.to_string());
    }
    if let Some(homepage) = str_field(info, "homepage") {
        lines.push(underline_blue(homepage));
    }
    if let Some(deprecated) = str_field(info, "deprecated") {
        lines.push(String::new());
        lines.push(format!("{} - {deprecated}", red("DEPRECATED!")));
    }

    if let Some(keywords) = array_field(info, "keywords") {
        let joined = keywords.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
        lines.push(String::new());
        lines.push(format!("keywords: {}", cyan(&joined)));
    }

    lines.extend(bin_summary(info));

    if let Some(dist) = info.get("dist").and_then(Value::as_object) {
        lines.push(String::new());
        lines.push(bold("dist"));
        if let Some(tarball) = obj_str(dist, "tarball") {
            lines.push(format!(".tarball: {}", underline_blue(tarball)));
        }
        if let Some(shasum) = obj_str(dist, "shasum") {
            lines.push(format!(".shasum: {}", green(shasum)));
        }
        if let Some(integrity) = obj_str(dist, "integrity") {
            lines.push(format!(".integrity: {}", green(integrity)));
        }
        if let Some(unpacked_size) = dist.get("unpackedSize").and_then(Value::as_u64) {
            lines.push(format!(".unpackedSize: {}", blue(&format_bytes(unpacked_size))));
        }
    }

    if let Some(dependencies) = info.get("dependencies").and_then(Value::as_object)
        && !dependencies.is_empty()
    {
        lines.push(String::new());
        lines.push("dependencies:".to_string());
        let entries: Vec<String> = dependencies
            .iter()
            .map(|(name, version)| {
                format!("{}: {}", blue(name), version.as_str().unwrap_or_default())
            })
            .collect();
        lines.push(entries.join(", "));
    }

    if let Some(maintainers) = array_field(info, "maintainers") {
        lines.push(String::new());
        lines.push("maintainers:".to_string());
        for maintainer in maintainers {
            lines.push(format!("- {}", format_person(maintainer)));
        }
    }

    if let Some(dist_tags) = info.get("distTags").and_then(Value::as_object)
        && !dist_tags.is_empty()
    {
        lines.push(String::new());
        lines.push(bold("dist-tags:"));
        for (tag, version) in dist_tags {
            lines.push(format!("{}: {}", blue(tag), version.as_str().unwrap_or_default()));
        }
    }

    if let Some(published) = published_info(info) {
        lines.push(String::new());
        lines.push(published);
    }

    lines.join("\n")
}

/// Render the `bin:` summary line(s). A string `bin` derives its single
/// command from the (scope-stripped) package name; an object `bin` lists
/// its keys.
fn bin_summary(info: &Value) -> Vec<String> {
    let bins: Vec<String> = match info.get("bin") {
        Some(Value::String(bin)) if !bin.is_empty() => match str_field(info, "name") {
            Some(name) if name.starts_with('@') => {
                vec![name.split_once('/').map_or(name, |(_, rest)| rest).to_string()]
            }
            Some(name) => vec![name.to_string()],
            None => Vec::new(),
        },
        Some(Value::Object(bin)) => bin.keys().cloned().collect(),
        _ => Vec::new(),
    };
    if bins.is_empty() {
        return Vec::new();
    }
    vec![String::new(), format!("bin: {}", cyan(&bins.join(", ")))]
}

/// Build the `published <time> ago[ by <publisher>]` line. Needs the picked
/// version's publish timestamp; an unparsable timestamp yields no line, and
/// a future one degrades to "just now".
fn published_info(info: &Value) -> Option<String> {
    let version = str_field(info, "version")?;
    let published_time = info.get("time")?.as_object()?.get(version)?.as_str()?;
    let date = parse_date(published_time)?;
    let time_ago = format_time_ago(date).unwrap_or_else(|| "just now".to_string());
    Some(match publisher(info) {
        Some(publisher) => format!("published {} by {publisher}", cyan(&time_ago)),
        None => format!("published {}", cyan(&time_ago)),
    })
}

/// Resolve the publisher shown in the published-by line, preferring
/// `_npmUser`, then the first maintainer (with `et al.` when there are
/// more), then the author.
fn publisher(info: &Value) -> Option<String> {
    if let Some(npm_user) = info.get("_npmUser").and_then(Value::as_object)
        && obj_str(npm_user, "name").is_some()
    {
        return Some(format_person(&Value::Object(npm_user.clone())));
    }
    if let Some(maintainers) = array_field(info, "maintainers") {
        let formatted = format_person(&maintainers[0]);
        return Some(if maintainers.len() == 1 {
            formatted
        } else {
            format!("{formatted} et al.")
        });
    }
    str_field(info, "author").map(ToString::to_string)
}

/// Format a `{ name, email }` person object as `name <email>` (blue name,
/// dimmed email), or just the name when no email is present.
fn format_person(person: &Value) -> String {
    let name = person.get("name").and_then(Value::as_str).unwrap_or("");
    match person.get("email").and_then(Value::as_str).filter(|email| !email.is_empty()) {
        Some(email) => format!("{} <{}>", blue(name), dim(email)),
        None => blue(name),
    }
}

/// Format a byte count with a 1000-based unit and at most two decimals.
fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    const SIZES: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    let bytes = bytes as f64;
    let index = bytes.log(1000_f64).floor() as usize;
    let index = index.min(SIZES.len() - 1);
    let value = (bytes / 1000_f64.powi(index as i32) * 100.0).round() / 100.0;
    format!("{value} {}", SIZES[index])
}

/// The age of `date` relative to `now`, bucketed into a coarse "N unit(s)
/// ago" label. `None` for a future date (clock skew). Split from
/// [`format_time_ago`] so the `now` reference is injectable in tests.
fn format_time_ago_since(date: DateTime<Utc>, now: DateTime<Utc>) -> Option<String> {
    let diff_ms = now.signed_duration_since(date).num_milliseconds();
    if diff_ms < 0 {
        return None;
    }
    let diff_sec = diff_ms / 1000;
    let diff_min = diff_sec / 60;
    let diff_hour = diff_min / 60;
    let diff_day = diff_hour / 24;
    let diff_month = diff_day / 30;
    let diff_year = diff_day / 365;
    let unit = |count: i64, singular: &str| {
        format!("{count} {singular}{} ago", if count == 1 { "" } else { "s" })
    };
    Some(if diff_year > 0 {
        unit(diff_year, "year")
    } else if diff_month > 0 {
        unit(diff_month, "month")
    } else if diff_day > 0 {
        unit(diff_day, "day")
    } else if diff_hour > 0 {
        unit(diff_hour, "hour")
    } else if diff_min > 0 {
        unit(diff_min, "minute")
    } else {
        "a few seconds ago".to_string()
    })
}

fn format_time_ago(date: DateTime<Utc>) -> Option<String> {
    format_time_ago_since(date, Utc::now())
}

fn parse_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|date| date.with_timezone(&Utc))
}

fn to_pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// A field's string value, treating an empty string as absent.
fn str_field<'a>(info: &'a Value, key: &str) -> Option<&'a str> {
    info.get(key).and_then(Value::as_str).filter(|value| !value.is_empty())
}

fn obj_str<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str).filter(|value| !value.is_empty())
}

/// A field's array value, treating an empty array as absent.
fn array_field<'a>(info: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    info.get(key).and_then(Value::as_array).filter(|array| !array.is_empty())
}

fn cyan(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::cyan).to_string()
}

fn green(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::green).to_string()
}

fn blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::blue).to_string()
}

fn red(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::red).to_string()
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::bold).to_string()
}

fn dim(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::dimmed).to_string()
}

fn underline_blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |text| text.style(Style::new().blue().underline()))
        .to_string()
}

#[cfg(test)]
mod tests;
