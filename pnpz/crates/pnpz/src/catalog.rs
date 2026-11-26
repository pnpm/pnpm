//! The catalog maps a package name and version to the store paths nixpkgs
//! built for it on each system.
//!
//! This proof of concept reads the public Nixhub index, which already answers
//! that question. The catalog pnpr is meant to publish replaces it.

use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_network::{
    ThrottledClient,
    encode_uri_component,
};
use pnpz_nix_store::StorePath;
use reqwest::StatusCode;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::Arc,
};

pub const DEFAULT_CATALOG_URL: &str = "https://search.devbox.sh";

/// Overrides [`DEFAULT_CATALOG_URL`].
pub const CATALOG_URL_ENV: &str = "PNPZ_CATALOG_URL";

pub struct Catalog {
    client: Arc<ThrottledClient>,
    base_url: String,
}

/// One version of a package, as the catalog resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPackage {
    pub version: String,
    /// Keyed by system. A system the package has no build for is absent.
    pub systems: BTreeMap<String, CatalogSystem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSystem {
    pub nixpkgs: String,
    pub attr: String,
    /// The outputs an install puts in the environment, keyed by output name.
    pub outputs: BTreeMap<String, StorePath>,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum CatalogError {
    #[display("the catalog has no package {name}@{specifier}")]
    #[diagnostic(code(ERR_PNPZ_PACKAGE_NOT_FOUND))]
    NotFound { name: String, specifier: String },
    #[display("the catalog request for {name}@{specifier} failed: {source}")]
    #[diagnostic(code(ERR_PNPZ_CATALOG_REQUEST))]
    Request { name: String, specifier: String, source: reqwest::Error },
    #[display("the catalog answered {status} for {name}@{specifier}")]
    #[diagnostic(code(ERR_PNPZ_CATALOG_STATUS))]
    Status { name: String, specifier: String, status: StatusCode },
    #[display("the catalog entry for {name}@{specifier} is invalid: {reason}")]
    #[diagnostic(code(ERR_PNPZ_CATALOG_INVALID))]
    Invalid { name: String, specifier: String, reason: String },
}

#[derive(Deserialize)]
struct ResolveResponse {
    version: String,
    #[serde(default)]
    systems: BTreeMap<String, SystemEntry>,
}

#[derive(Deserialize)]
struct SystemEntry {
    flake_installable: FlakeInstallable,
    #[serde(default)]
    outputs: Vec<OutputEntry>,
}

#[derive(Deserialize)]
struct FlakeInstallable {
    #[serde(rename = "ref")]
    reference: FlakeReference,
    attr_path: String,
}

#[derive(Deserialize)]
struct FlakeReference {
    rev: String,
}

#[derive(Deserialize)]
struct OutputEntry {
    name: String,
    path: String,
    #[serde(default)]
    default: bool,
}

impl Catalog {
    pub fn new(client: Arc<ThrottledClient>, base_url: &str) -> Self {
        Self { client, base_url: base_url.trim_end_matches('/').to_string() }
    }

    /// Resolves `specifier` (an exact version, a version prefix, or `latest`)
    /// to one version of the package `name`.
    pub async fn resolve(
        &self,
        name: &str,
        specifier: &str,
    ) -> Result<CatalogPackage, CatalogError> {
        let url = format!(
            "{}/v2/resolve?name={}&version={}",
            self.base_url,
            encode_uri_component(name),
            encode_uri_component(specifier),
        );
        let (name, specifier) = (name.to_string(), specifier.to_string());
        let client = self.client.acquire_for_url(&url).await;
        let response = match client.get(&url).send().await {
            Ok(response) => response,
            Err(source) => return Err(CatalogError::Request { name, specifier, source }),
        };
        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(CatalogError::NotFound { name, specifier }),
            status => return Err(CatalogError::Status { name, specifier, status }),
        }
        match response.json::<ResolveResponse>().await {
            Ok(body) => package_from_response(body)
                .map_err(|reason| CatalogError::Invalid { name, specifier, reason }),
            Err(source) => Err(CatalogError::Request { name, specifier, source }),
        }
    }
}

fn package_from_response(body: ResolveResponse) -> Result<CatalogPackage, String> {
    let systems = body.systems
        .into_iter()
        .map(|(system, entry)| Ok((system, system_from_entry(entry)?)))
        .collect::<Result<_, String>>()?;
    Ok(CatalogPackage { version: body.version, systems })
}

/// Keeps the outputs the catalog marks as installed by default. A package
/// that marks none gets all of them.
fn system_from_entry(entry: SystemEntry) -> Result<CatalogSystem, String> {
    let has_default = entry.outputs.iter().any(|output| output.default);
    let outputs = entry.outputs
        .into_iter()
        .filter(|output| output.default || !has_default)
        .map(|output| match StorePath::from_absolute(&output.path) {
            Ok(path) => Ok((output.name, path)),
            Err(error) => Err(error.to_string()),
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    if outputs.is_empty() {
        return Err("a system lists no outputs".to_string());
    }
    Ok(CatalogSystem {
        nixpkgs: entry.flake_installable.reference.rev,
        attr: entry.flake_installable.attr_path,
        outputs,
    })
}
