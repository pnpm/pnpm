//! The Python package index protocol pnpr speaks.
//!
//! A Python index is the **Simple Repository API**: a project list and one
//! page per project listing its distribution files, served as HTML (PEP 503)
//! or JSON (PEP 691 / PEP 700), plus the **legacy upload API** that `twine`
//! and build front ends POST a `multipart/form-data` file to. This crate
//! holds the protocol pieces the server needs and nothing about storage or
//! routing: project-name normalization, the project document and its two
//! renderings, distribution-filename parsing, and the upload form.

pub mod multipart;

use derive_more::{Display, Error};
use pep440_rs::Version;
use pep508_rs::PackageName;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt::Write as _, str::FromStr};

/// The URL segment under a registry endpoint where the Simple API lives:
/// `/~<name>/simple/`, `/~<name>/simple/<project>/`.
pub const SIMPLE_PATH: &str = "simple";

/// The URL segment under a registry endpoint that distribution files are
/// served from: `/~<name>/files/<project>/<filename>`.
pub const FILES_PATH: &str = "files";

/// The URL segment under a registry endpoint that accepts uploads:
/// `POST /~<name>/legacy/`.
pub const UPLOAD_PATH: &str = "legacy";

/// The PEP 691 JSON content type of a Simple API page.
pub const JSON_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+json";

/// The versioned HTML content type of a Simple API page. Plain `text/html`
/// is served to clients that ask for nothing more specific.
pub const HTML_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+html";

/// The Simple API version the JSON pages declare. `1.1` adds the PEP 700
/// `versions`, `size` and `upload-time` keys.
pub const API_VERSION: &str = "1.1";

/// A project name no Python index can serve.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("{name:?} is not a valid Python project name")]
pub struct NameError {
    pub name: String,
}

/// Normalize a project name the PEP 503 way (lowercase, runs of `-`, `_`
/// and `.` collapsed to one `-`), rejecting names PEP 508 does not allow.
/// Every project name in a URL, an upload form, or a filename passes
/// through here, so `Demo_Pkg`, `demo.pkg` and `demo-pkg` are one project.
pub fn normalize_name(raw: &str) -> Result<String, NameError> {
    let invalid = || NameError { name: raw.to_string() };
    let normalized = PackageName::from_str(raw).map_err(|_| invalid())?.as_ref().to_string();
    let well_formed =
        normalized.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
            && normalized.chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric())
            && normalized.chars().next_back().is_some_and(|ch| ch.is_ascii_alphanumeric());
    well_formed.then_some(normalized).ok_or_else(invalid)
}

/// A version string PEP 440 does not allow.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("{version:?} is not a valid Python version")]
pub struct VersionError {
    pub version: String,
}

/// Parse a PEP 440 version and return its normalized spelling.
pub fn normalize_version(raw: &str) -> Result<String, VersionError> {
    Version::from_str(raw)
        .map(|version| version.to_string())
        .map_err(|_| VersionError { version: raw.to_string() })
}

/// Whether a file has been yanked (PEP 592): a flag, or the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Yanked {
    Flag(bool),
    Reason(String),
}

impl Default for Yanked {
    fn default() -> Self {
        Self::Flag(false)
    }
}

impl Yanked {
    #[must_use]
    pub fn is_yanked(&self) -> bool {
        !matches!(self, Yanked::Flag(false))
    }
}

/// One distribution file of a project, in the PEP 691 key spelling.
///
/// `url` is the file's origin URL as an upstream index published it, and is
/// absent in a hosted document, where the server derives every file's URL
/// from its own address at render time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProjectFile {
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Lowercase hash algorithm name to lowercase hex digest. A file must
    /// carry a `sha256`.
    #[serde(default)]
    pub hashes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_python: Option<String>,
    #[serde(default)]
    pub yanked: Yanked,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_time: Option<String>,
}

impl ProjectFile {
    /// The file's lowercase hex SHA-256, when the index published one.
    #[must_use]
    pub fn sha256(&self) -> Option<&str> {
        self.hashes.get("sha256").map(String::as_str)
    }
}

/// A project's file listing: the document pnpr stores per hosted project,
/// and the shape it reads an upstream PEP 691 page into (the page's extra
/// keys are ignored).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDocument {
    pub name: String,
    #[serde(default)]
    pub files: Vec<ProjectFile>,
}

impl ProjectDocument {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), files: Vec::new() }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("project document serializes")
    }

    #[must_use]
    pub fn file(&self, filename: &str) -> Option<&ProjectFile> {
        self.files.iter().find(|file| file.filename == filename)
    }

    /// The distinct versions the listed files belong to, oldest first, for
    /// the PEP 700 `versions` key. Files whose names do not parse are
    /// skipped rather than failing the page.
    #[must_use]
    pub fn versions(&self) -> Vec<String> {
        let mut versions: Vec<Version> = self
            .files
            .iter()
            .filter_map(|file| parse_distribution_filename(&file.filename).ok())
            .filter_map(|distribution| Version::from_str(&distribution.version).ok())
            .collect();
        versions.sort();
        versions.dedup();
        versions.iter().map(ToString::to_string).collect()
    }

    /// The PEP 691 JSON page, with every file served from
    /// `<file_base>/<filename>`.
    #[must_use]
    pub fn render_json(&self, file_base: &str) -> Value {
        let files: Vec<Value> = self
            .files
            .iter()
            .map(|file| {
                let mut entry = json!({
                    "filename": file.filename,
                    "url": file_url(file_base, &file.filename),
                    "hashes": file.hashes,
                    "yanked": file.yanked,
                });
                if let Some(requires_python) = &file.requires_python {
                    entry["requires-python"] = json!(requires_python);
                }
                if let Some(size) = file.size {
                    entry["size"] = json!(size);
                }
                if let Some(upload_time) = &file.upload_time {
                    entry["upload-time"] = json!(upload_time);
                }
                entry
            })
            .collect();
        json!({
            "meta": { "api-version": API_VERSION },
            "name": self.name,
            "versions": self.versions(),
            "files": files,
        })
    }

    /// The PEP 503 HTML page, with every file served from
    /// `<file_base>/<filename>` and its SHA-256 in the URL fragment.
    #[must_use]
    pub fn render_html(&self, file_base: &str) -> String {
        let mut html = String::new();
        let _ = write!(
            html,
            "<!DOCTYPE html>\n<html>\n<head>\n<meta name=\"pypi:repository-version\" \
             content=\"1.1\">\n<title>Links for {name}</title>\n</head>\n<body>\n<h1>Links for \
             {name}</h1>\n",
            name = escape_html(&self.name),
        );
        for file in &self.files {
            let mut href = file_url(file_base, &file.filename);
            if let Some(sha256) = file.sha256() {
                let _ = write!(href, "#sha256={sha256}");
            }
            let _ = write!(html, r#"<a href="{}""#, escape_html(&href));
            if let Some(requires_python) = &file.requires_python {
                let _ = write!(html, r#" data-requires-python="{}""#, escape_html(requires_python));
            }
            match &file.yanked {
                Yanked::Flag(false) => {}
                Yanked::Flag(true) => html.push_str(r#" data-yanked="""#),
                Yanked::Reason(reason) => {
                    let _ = write!(html, r#" data-yanked="{}""#, escape_html(reason));
                }
            }
            let _ = writeln!(html, ">{}</a><br />", escape_html(&file.filename));
        }
        html.push_str("</body>\n</html>\n");
        html
    }
}

fn file_url(file_base: &str, filename: &str) -> String {
    format!("{}/{}", file_base.trim_end_matches('/'), pnpm_network::encode_uri_component(filename))
}

/// The PEP 691 JSON project list, with every project page at
/// `<simple_base>/<name>/`.
#[must_use]
pub fn render_project_list_json<'name>(names: impl IntoIterator<Item = &'name str>) -> Value {
    let projects: Vec<Value> = names.into_iter().map(|name| json!({ "name": name })).collect();
    json!({ "meta": { "api-version": API_VERSION }, "projects": projects })
}

/// The PEP 503 HTML project list, with every project page at
/// `<simple_base>/<name>/`.
#[must_use]
pub fn render_project_list_html<'name>(
    simple_base: &str,
    names: impl IntoIterator<Item = &'name str>,
) -> String {
    let mut html = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta name=\"pypi:repository-version\" \
         content=\"1.1\">\n<title>Simple index</title>\n</head>\n<body>\n",
    );
    for name in names {
        let _ = writeln!(
            html,
            r#"<a href="{}/{}/">{}</a><br />"#,
            escape_html(simple_base.trim_end_matches('/')),
            escape_html(name),
            escape_html(name),
        );
    }
    html.push_str("</body>\n</html>\n");
    html
}

/// Escape text for an HTML text node or a double-quoted attribute value.
#[must_use]
pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Whether a request's `Accept` header prefers the PEP 691 JSON page over
/// HTML: the JSON type is listed with a non-zero quality at least as high as
/// any HTML type's. `*/*` alone does not select JSON, since browsers and
/// unaware clients send it.
#[must_use]
pub fn wants_json(accept: Option<&str>) -> bool {
    let Some(accept) = accept else { return false };
    let json = quality(accept, JSON_CONTENT_TYPE);
    let html = [HTML_CONTENT_TYPE, "text/html"]
        .into_iter()
        .filter_map(|media| media_quality(accept, media))
        .fold(0.0_f32, f32::max);
    json.is_some_and(|json| json > 0.0 && json >= html)
}

/// Whether the versioned HTML type has positive quality at least as high
/// as plain HTML in the request's `Accept` header.
#[must_use]
pub fn wants_versioned_html(accept: Option<&str>) -> bool {
    accept.is_some_and(|accept| {
        quality(accept, HTML_CONTENT_TYPE).is_some_and(|quality| {
            quality > 0.0 && quality >= media_quality(accept, "text/html").unwrap_or(0.0)
        })
    })
}

fn media_quality(accept: &str, media: &str) -> Option<f32> {
    let (kind, _) = media.split_once('/')?;
    quality(accept, media)
        .or_else(|| quality(accept, &format!("{kind}/*")))
        .or_else(|| quality(accept, "*/*"))
}

/// The quality an `Accept` header assigns to exactly `media`, `None` when
/// the type is not listed. A listed type without `q` has quality 1.
fn quality(accept: &str, media: &str) -> Option<f32> {
    accept
        .split(',')
        .filter_map(|entry| {
            let mut parameters = entry.split(';');
            let listed = parameters.next()?.trim();
            if !listed.eq_ignore_ascii_case(media) {
                return None;
            }
            let weight = parameters
                .filter_map(|parameter| parameter.trim().split_once('='))
                .find(|(name, _)| name.trim().eq_ignore_ascii_case("q"))
                .map_or(1.0, |(_, value)| {
                    value
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|weight| (0.0..=1.0).contains(weight))
                        .unwrap_or(0.0)
                });
            Some(weight)
        })
        .fold(None, |best: Option<f32>, q| Some(best.map_or(q, |best| best.max(q))))
}

/// The kind of distribution a filename denotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionKind {
    Wheel,
    Sdist,
}

/// The project and version a distribution filename encodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distribution {
    /// The normalized project name.
    pub name: String,
    /// The version as spelled in the filename.
    pub version: String,
    pub kind: DistributionKind,
}

/// A filename that is not a wheel or sdist.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("{filename:?} is not a wheel or source distribution filename")]
pub struct FilenameError {
    pub filename: String,
}

/// Parse a wheel (`name-version(-build)?-py-abi-platform.whl`) or sdist
/// (`name-version.tar.gz` / `.zip`) filename. The name is normalized and
/// the version must be PEP 440.
pub fn parse_distribution_filename(filename: &str) -> Result<Distribution, FilenameError> {
    let invalid = || FilenameError { filename: filename.to_string() };
    if filename.contains(['/', '\\']) {
        return Err(invalid());
    }
    let (name, version, kind) = if let Some(stem) = filename.strip_suffix(".whl") {
        let parts: Vec<&str> = stem.split('-').collect();
        if !(parts.len() == 5 || parts.len() == 6) {
            return Err(invalid());
        }
        (parts[0], parts[1], DistributionKind::Wheel)
    } else if let Some(stem) =
        filename.strip_suffix(".tar.gz").or_else(|| filename.strip_suffix(".zip"))
    {
        let (name, version) = stem.rsplit_once('-').ok_or_else(invalid)?;
        (name, version, DistributionKind::Sdist)
    } else {
        return Err(invalid());
    };
    let name = normalize_name(name).map_err(|_| invalid())?;
    Version::from_str(version).map_err(|_| invalid())?;
    Ok(Distribution { name, version: version.to_string(), kind })
}

/// A legacy-API upload request that cannot be accepted.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
pub enum UploadError {
    #[display("upload form field `:action` must be `file_upload`")]
    NotAFileUpload,
    #[display("upload form field `protocol_version` must be `1`")]
    UnsupportedProtocolVersion,
    #[display("upload form is missing the `{_0}` field")]
    MissingField(#[error(not(source))] &'static str),
    #[display("upload form field `{_0}` is not valid UTF-8")]
    NotText(#[error(not(source))] &'static str),
    #[display("upload form field `content` carries no filename")]
    MissingFilename,
}

/// The fields of a legacy-API file upload the server acts on. Metadata
/// fields beyond these are accepted and ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub name: String,
    pub version: String,
    /// `bdist_wheel` or `sdist`.
    pub filetype: String,
    pub filename: String,
    pub content: Vec<u8>,
    pub sha256_digest: Option<String>,
    pub requires_python: Option<String>,
}

/// Read a legacy-API upload out of its parsed `multipart/form-data` parts.
pub fn parse_upload(parts: Vec<multipart::FormPart>) -> Result<Upload, UploadError> {
    let mut fields: BTreeMap<String, multipart::FormPart> = BTreeMap::new();
    for part in parts {
        fields.entry(part.name.clone()).or_insert(part);
    }
    let text = |fields: &BTreeMap<String, multipart::FormPart>, name: &'static str| {
        fields
            .get(name)
            .map(|part| {
                String::from_utf8(part.data.clone()).map_err(|_| UploadError::NotText(name))
            })
            .transpose()
    };
    if text(&fields, ":action")?.as_deref() != Some("file_upload") {
        return Err(UploadError::NotAFileUpload);
    }
    if text(&fields, "protocol_version")?.is_some_and(|version| version != "1") {
        return Err(UploadError::UnsupportedProtocolVersion);
    }
    let name = text(&fields, "name")?.ok_or(UploadError::MissingField("name"))?;
    let version = text(&fields, "version")?.ok_or(UploadError::MissingField("version"))?;
    let filetype = text(&fields, "filetype")?.ok_or(UploadError::MissingField("filetype"))?;
    let sha256_digest = text(&fields, "sha256_digest")?.filter(|digest| !digest.is_empty());
    let requires_python = text(&fields, "requires_python")?.filter(|value| !value.is_empty());
    let content = fields.remove("content").ok_or(UploadError::MissingField("content"))?;
    let filename = content.filename.ok_or(UploadError::MissingFilename)?;
    Ok(Upload {
        name,
        version,
        filetype,
        filename,
        content: content.data,
        sha256_digest: sha256_digest.map(|digest| digest.to_ascii_lowercase()),
        requires_python,
    })
}

#[cfg(test)]
mod tests;
