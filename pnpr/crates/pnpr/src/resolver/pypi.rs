//! Python resolution for the install accelerator.
//!
//! `POST /-/pnpr/v0/resolve` with `"ecosystem": "pypi"` resolves a Python
//! project the way the Cargo surface next door resolves a Cargo one: the
//! client sends what only it has — its requirements and the interpreter
//! they are for — and the server does the part that costs round trips.
//!
//! For Python that part is most of the work. Resolving a requirement reads
//! a distribution's `METADATA`, which lives inside a wheel, so a client
//! resolving alone downloads whole wheels for versions it may then reject.
//! A server reads the metadata file an index publishes beside each wheel
//! (PEP 658), falling back to the wheel itself only when the index
//! publishes none, and it keeps what it read for every client that follows.
//!
//! The interpreter never enters into it: the marker environment and the
//! wheel tags travel in the request as the client's own, and pnpr matches
//! wheels against them without running any Python.
//!
//! What comes back is a `pylock.toml` document. The client writes it, then
//! downloads the wheels it names and checks them against the digests the
//! index published — and re-solves the project against the metadata of
//! what it actually downloaded, so a wrong answer here cannot become an
//! installed environment.

use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use axum::{http::StatusCode, response::Response};
use pnpm_network::{AuthHeaders, MetadataCacheScope, RetryOpts, ThrottledClient};
use pnpm_python_resolver::{
    Candidate, Inputs, Lockfile, Packages, Step, Target, WheelMetadata, candidates_from_page,
    parse_requirement, validate_url,
};
use pnpr_route::{Footprint, url_has_inline_credentials};

use crate::server::StripedLocks;

use super::{
    Resolver, json_error,
    package_route::PackageRoute,
    protocol::PypiResolveRequest,
    request_validation::forbidden_off_allowlist,
    wire::{error_frame, ndjson_single_frame, pypi_done_frame},
};

/// How many distributions one resolve may read index pages for. A Python
/// project reaches a few hundred at the top of the scale; past this the
/// request is walking an index rather than resolving a project.
const MAX_DISTRIBUTIONS: usize = 5_000;

/// How many wheels one resolve may read metadata for. Resolution reads
/// more versions than it keeps, so this is deliberately well above
/// [`MAX_DISTRIBUTIONS`].
const MAX_METADATA_READS: usize = 20_000;

/// Cap on a single Simple API page.
const MAX_PAGE_BYTES: usize = 32 * 1024 * 1024;

/// Cap on a single `METADATA` document.
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

/// Cap on a wheel read for its metadata, which only an index that
/// publishes no metadata files forces.
const MAX_WHEEL_BYTES: usize = 256 * 1024 * 1024;

/// Cap on the index bytes one resolve holds, as the Cargo path bounds the
/// sparse-index bytes one of its own holds. A read is charged when it
/// lands and none starts once the budget is spent.
const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;

/// Handle an `"ecosystem": "pypi"` resolve request: read what the project
/// needs from the index, solve it, and answer with the `pylock.toml` the
/// client writes.
pub(super) async fn handle_resolve(
    runtime: &Resolver,
    identity: pnpr_policy::Identity,
    body: &[u8],
) -> Response {
    let request: PypiResolveRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };
    let index = match index_url(&request.index) {
        Ok(index) => index,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err),
    };
    if url_has_inline_credentials(index.as_str()) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "inline URL credentials (user:pass@host) are not allowed; \
             configure an upstream credential alias instead",
        );
    }
    if !runtime.route_context.allows_registry(index.as_str()) {
        return forbidden_off_allowlist(index.as_str());
    }
    let requirements = match request
        .requirements
        .iter()
        .map(|requirement| parse_requirement(requirement))
        .collect::<miette::Result<Vec<_>>>()
    {
        Ok(requirements) => requirements,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &super::report_message(&err)),
    };

    let reader = IndexReader {
        client: Arc::clone(&runtime.client),
        route: Arc::clone(&runtime.route_context),
        identity,
        footprint: Arc::new(Mutex::new(Footprint::default())),
        secret: Arc::clone(&runtime.resolution_cache_secret),
        locks: Arc::clone(&runtime.python_index_locks),
        cache_dir: runtime.python_index_cache_dir(index.as_str()),
        ttl: runtime.cargo_index_ttl,
        bytes_held: AtomicUsize::new(0),
        index,
    };
    let inputs = Inputs::new(&requirements, &request.target, reader.index.as_str());
    match resolve(&reader, &requirements, &request.target).await {
        Ok(packages) => {
            let solution = packages.0;
            match Lockfile::new(
                &packages.1,
                &request.target,
                solution,
                inputs,
                request.requires_python,
            ) {
                Ok(lockfile) => ndjson_single_frame(&pypi_done_frame(&lockfile)),
                Err(err) => ndjson_single_frame(&error_frame(&super::report_message(&err))),
            }
        }
        Err(err) => ndjson_single_frame(&error_frame(&err)),
    }
}

type Solved = (BTreeMap<pep508_rs::PackageName, pep440_rs::Version>, Packages);

/// Feed the resolver what it asks for until the project is solved: an
/// index page for a distribution it has not seen, or one wheel's
/// `METADATA`.
async fn resolve(
    reader: &IndexReader,
    requirements: &[pep508_rs::Requirement],
    target: &Target,
) -> Result<Solved, String> {
    let mut packages = Packages::new();
    loop {
        let step = pnpm_python_resolver::step(&packages, requirements, &target.environment)
            .map_err(|err| super::report_message(&err))?;
        match step {
            Step::Solved(solution) => return Ok((solution, packages)),
            Step::NeedCandidates(name) => {
                if packages.candidates.len() >= MAX_DISTRIBUTIONS {
                    return Err(format!(
                        "resolving this project needs more than {MAX_DISTRIBUTIONS} distributions",
                    ));
                }
                let candidates = reader.candidates(&name, target).await?;
                packages.candidates.insert(name, candidates);
            }
            Step::NeedMetadata(name, version) => {
                if packages.metadata.len() >= MAX_METADATA_READS {
                    return Err(format!(
                        "resolving this project needs the metadata of more than \
                         {MAX_METADATA_READS} wheels",
                    ));
                }
                let candidate = packages
                    .candidates
                    .get(&name)
                    .and_then(|versions| versions.get(&version))
                    .ok_or_else(|| format!("{name} {version} is not a candidate"))?;
                let metadata = reader.metadata(&name, candidate).await?;
                packages.metadata.insert((name, version), metadata);
            }
        }
        tokio::task::yield_now().await;
    }
}

/// Reads a Python index for one resolve: cache first, then the index.
struct IndexReader {
    client: Arc<ThrottledClient>,
    route: Arc<pnpr_route::RouteContext>,
    identity: pnpr_policy::Identity,
    footprint: Arc<Mutex<Footprint>>,
    secret: Arc<[u8]>,
    locks: Arc<StripedLocks>,
    cache_dir: PathBuf,
    ttl: Duration,
    bytes_held: AtomicUsize,
    /// The Simple API base URL, with the trailing slash a project page is
    /// resolved against.
    index: url::Url,
}

impl IndexReader {
    /// The versions of `name` this target can install, from the index's
    /// project page.
    async fn candidates(
        &self,
        name: &pep508_rs::PackageName,
        target: &Target,
    ) -> Result<BTreeMap<pep440_rs::Version, Candidate>, String> {
        let page_url = self
            .index
            .join(&format!("{name}/"))
            .map_err(|err| format!("build the index URL for {name}: {err}"))?;
        let canonical_name = canonical_project_name(name)?;
        let (page, page_url) = self
            .read(
                &page_url,
                &canonical_name,
                "page",
                MAX_PAGE_BYTES,
                Some(pnpr_pypi::JSON_CONTENT_TYPE),
            )
            .await?;
        candidates_from_page(&page, &page_url, name, target)
            .map_err(|err| super::report_message(&err))
    }

    /// One wheel's `METADATA`, from the file the index publishes beside it
    /// when it publishes one, and out of the wheel itself when it does not.
    async fn metadata(
        &self,
        name: &pep508_rs::PackageName,
        candidate: &Candidate,
    ) -> Result<WheelMetadata, String> {
        let canonical_name = canonical_project_name(name)?;
        let wheel_url = url::Url::parse(&candidate.wheel.url)
            .map_err(|err| format!("parse the wheel URL for {name}: {err}"))?;
        validate_url(&wheel_url).map_err(|err| super::report_message(&err))?;
        if let Some(digests) = &candidate.core_metadata {
            let metadata_url = format!("{}.metadata", candidate.wheel.url);
            let metadata_url = url::Url::parse(&metadata_url)
                .map_err(|err| format!("build the metadata URL for {name}: {err}"))?;
            let (document, _) = self
                .read(&metadata_url, &canonical_name, "metadata", MAX_METADATA_BYTES, None)
                .await?;
            verify_digest(document.as_bytes(), digests, "metadata file", &candidate.wheel.name)?;
            return WheelMetadata::parse(&document).map_err(|err| super::report_message(&err));
        }
        let (wheel, _) =
            self.read_bytes(&wheel_url, &canonical_name, "wheel", MAX_WHEEL_BYTES, None).await?;
        verify_digest(&wheel, &candidate.wheel.hashes, "wheel", &candidate.wheel.name)?;
        let document = metadata_from_wheel(&wheel, &candidate.wheel.name)?;
        WheelMetadata::parse(&document).map_err(|err| super::report_message(&err))
    }

    /// Read a document, from the cache when it is still fresh and from the
    /// index otherwise. Returns the document and the URL it was read from,
    /// which relative links in it resolve against.
    async fn read(
        &self,
        url: &url::Url,
        canonical_name: &str,
        kind: &str,
        limit: usize,
        accept: Option<&str>,
    ) -> Result<(String, url::Url), String> {
        let (bytes, source) = self.read_bytes(url, canonical_name, kind, limit, accept).await?;
        let document =
            String::from_utf8(bytes).map_err(|err| format!("decode the {kind} at {url}: {err}"))?;
        Ok((document, source))
    }

    async fn read_bytes(
        &self,
        url: &url::Url,
        canonical_name: &str,
        kind: &str,
        limit: usize,
        accept: Option<&str>,
    ) -> Result<(Vec<u8>, url::Url), String> {
        let auth = self.auth_for(canonical_name);
        let cache_path = self.cache_path(&auth, url);
        if let Some(cached) = self.cached(&cache_path).await {
            let source = cached.url(url)?;
            return Ok((self.hold(kind, cached.body)?, source));
        }
        let _reading = self.locks.lock(&cache_path.to_string_lossy()).await;
        if let Some(cached) = self.cached(&cache_path).await {
            let source = cached.url(url)?;
            return Ok((self.hold(kind, cached.body)?, source));
        }
        if !within_budget(self.bytes_held.load(Ordering::Relaxed)) {
            return Err(budget_exhausted(kind));
        }
        if !auth.allows_fetch(url.as_str()) {
            return Err(format!(
                "{url} is not allowed by this pnpr server; the operator must declare its \
                 registry as a public route or an upstream",
            ));
        }
        let response = self
            .client
            .get_limited_bytes_with_secure_auth_and_retry(
                url.as_str(),
                &auth,
                accept,
                RetryOpts::default(),
                limit,
            )
            .await
            .map_err(|err| format!("fetch the {kind} at {url}: {err}"))?;
        if response.body_truncated {
            return Err(format!("the {kind} at {url} exceeds {limit} bytes"));
        }
        if !response.status.is_success() {
            return Err(format!("fetch the {kind} at {url} returned HTTP {}", response.status));
        }
        let source = url::Url::parse(&response.url)
            .map_err(|err| format!("parse the URL the {kind} was read from: {err}"))?;
        let body = self.hold(kind, response.body)?;
        Self::store(cache_path, CachedDocument { url: source.to_string(), body: body.clone() })
            .await;
        Ok((body, source))
    }

    /// Account bytes against this resolve's budget, which bounds what one
    /// request can make the server hold and cache.
    fn hold(&self, kind: &str, body: Vec<u8>) -> Result<Vec<u8>, String> {
        let held = self.bytes_held.fetch_add(body.len(), Ordering::Relaxed) + body.len();
        if held > MAX_TOTAL_BYTES {
            return Err(budget_exhausted(kind));
        }
        Ok(body)
    }

    /// The request auth for a read about `canonical_name`: this server's
    /// route policy for the caller, with the project bound in so the
    /// package-blind fetch helpers still classify by it.
    fn auth_for(&self, canonical_name: &str) -> AuthHeaders {
        let hook = pnpr_route::RouteHook::new(
            Arc::clone(&self.route),
            self.identity.clone(),
            Arc::clone(&self.footprint),
            Arc::clone(&self.secret),
        );
        AuthHeaders::default()
            .with_route_hook(Arc::new(PackageRoute::new(hook, canonical_name.to_string())))
    }

    /// Where `url`'s document is cached. The route scope keys the
    /// namespace, so a private index cached under one caller's credential
    /// is never read back for a caller who does not reproduce that scope.
    fn cache_path(&self, auth: &AuthHeaders, url: &url::Url) -> PathBuf {
        let scope = match auth.metadata_scope(url.as_str(), None) {
            MetadataCacheScope::Public => "public".to_string(),
            MetadataCacheScope::Private { descriptor_id } => descriptor_id,
        };
        self.cache_dir
            .join(scope)
            .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(url.as_str())))
    }

    async fn cached(&self, path: &Path) -> Option<CachedDocument> {
        let metadata = tokio::fs::metadata(path).await.ok()?;
        let age = SystemTime::now().duration_since(metadata.modified().ok()?).ok()?;
        if age >= self.ttl {
            return None;
        }
        let bytes = tokio::fs::read(path).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Cache a document, best effort: a cache that cannot be written costs
    /// a refetch on the next resolve, which is not worth failing over.
    async fn store(path: PathBuf, document: CachedDocument) {
        let _ = tokio::task::spawn_blocking(move || {
            let parent = path.parent()?;
            std::fs::create_dir_all(parent).ok()?;
            let bytes = serde_json::to_vec(&document).ok()?;
            pnpm_fs::write_atomic(&path, &bytes).ok()
        })
        .await;
    }
}

/// A document as it was read, beside the URL it came from: a redirected
/// page's links resolve against where it landed, not where it was asked
/// for.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedDocument {
    url: String,
    body: Vec<u8>,
}

impl CachedDocument {
    fn url(&self, requested: &url::Url) -> Result<url::Url, String> {
        url::Url::parse(&self.url)
            .map_err(|err| format!("parse the cached URL of {requested}: {err}"))
    }
}

/// Check what was read against the SHA-256 the index published for it.
///
/// Resolution decides which versions the client will install, so a
/// metadata document that is not the one the index vouched for must not
/// reach the solver. An index that published no SHA-256 for a file leaves
/// nothing to check here; the client checks the wheels it downloads
/// against the digests in the lockfile regardless.
fn verify_digest(
    bytes: &[u8],
    digests: &BTreeMap<String, String>,
    kind: &str,
    filename: &str,
) -> Result<(), String> {
    let Some(expected) = digests.get("sha256") else { return Ok(()) };
    let actual = pnpm_crypto_hash::create_hex_hash_bytes(bytes);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "the {kind} of {filename} does not match the SHA-256 the index published",
        ));
    }
    Ok(())
}

/// The `METADATA` inside a wheel, for an index that publishes no metadata
/// file of its own.
fn metadata_from_wheel(wheel: &[u8], filename: &str) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(wheel))
        .map_err(|err| format!("read the wheel {filename}: {err}"))?;
    let entry = (0..archive.len())
        .filter_map(|index| Some(archive.by_index(index).ok()?.name().to_string()))
        .find(|name| {
            let mut segments = name.split('/');
            segments.next().is_some_and(|directory| directory.ends_with(".dist-info"))
                && segments.next() == Some("METADATA")
                && segments.next().is_none()
        })
        .ok_or_else(|| format!("the wheel {filename} has no dist-info METADATA"))?;
    let mut document = String::new();
    archive
        .by_name(&entry)
        .map_err(|err| format!("read {entry} from {filename}: {err}"))?
        .take(MAX_METADATA_BYTES as u64)
        .read_to_string(&mut document)
        .map_err(|err| format!("read {entry} from {filename}: {err}"))?;
    Ok(document)
}

/// The index base URL a request names, with the trailing slash a project
/// page is resolved against.
fn index_url(index: &str) -> Result<url::Url, String> {
    let mut url: url::Url =
        index.parse().map_err(|err| format!("parse the Python index URL: {err}"))?;
    validate_url(&url).map_err(|err| super::report_message(&err))?;
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

/// The PEP 503 spelling of a project's name, which is what the Python
/// registry surface matches its rules against.
fn canonical_project_name(name: &pep508_rs::PackageName) -> Result<String, String> {
    pnpr_pypi::normalize_name(name.as_ref()).map_err(|err| err.to_string())
}

fn within_budget(held: usize) -> bool {
    held < MAX_TOTAL_BYTES
}

fn budget_exhausted(kind: &str) -> String {
    format!(
        "resolving this project needs more than {MAX_TOTAL_BYTES} bytes of index metadata \
         (reached while reading a {kind})",
    )
}

#[cfg(test)]
mod tests;
