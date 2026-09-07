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

/// How many requirements one request may name. A project's manifest
/// lists tens; past this the request is not one a manifest produced.
const MAX_REQUIREMENTS: usize = 10_000;

/// How many wheel tags a target may list. An interpreter reports a few
/// hundred, and every tag is compared against every wheel filename, so an
/// unbounded list is work a caller can hand the server.
const MAX_TAGS: usize = 2_000;

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
/// publishes no metadata files forces. Well above an ordinary wheel: the
/// giant ones are published to indexes that do serve metadata files, so
/// this bounds the fallback without reaching for it.
const MAX_WHEEL_BYTES: usize = 64 * 1024 * 1024;

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
    if request.requirements.len() > MAX_REQUIREMENTS {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("a resolve request may name at most {MAX_REQUIREMENTS} requirements"),
        );
    }
    if request.target.tags.len() > MAX_TAGS {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("a resolve request may name at most {MAX_TAGS} wheel tags"),
        );
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
                let metadata = reader.metadata(&name, &version, candidate).await?;
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
        let canonical_name = canonical_project_name(name)?;
        let page_url = project_page_url(&self.index, &canonical_name)?;
        let auth = self.auth_for(&canonical_name);
        let cache_path = self.cache_path(&auth, &page_url, None);
        if let Some(cached) = self.cached(&cache_path).await {
            let source = cached.url(&page_url)?;
            let page = self.hold("page", cached.body)?;
            return parse_page(&page, &source, name, target);
        }
        let _reading = self.locks.lock(&cache_path.to_string_lossy()).await;
        if let Some(cached) = self.cached(&cache_path).await {
            let source = cached.url(&page_url)?;
            let page = self.hold("page", cached.body)?;
            return parse_page(&page, &source, name, target);
        }

        let (page, source) = self
            .fetch(&auth, &page_url, "page", MAX_PAGE_BYTES, Some(pnpr_pypi::JSON_CONTENT_TYPE))
            .await?;
        let page = text(page, "project page", name.as_ref())?;
        // Parsed before it is cached, so a page that is not one is not
        // served to every resolve that follows for the whole TTL.
        let candidates = parse_page(&page, &source, name, target)?;
        Self::store(cache_path, CachedDocument { url: source.to_string(), body: page }).await;
        Ok(candidates)
    }

    /// One wheel's `METADATA`, from the file the index publishes beside it
    /// when it publishes one, and out of the wheel itself when it does not.
    ///
    /// What is cached is the metadata document either way, so a wheel is
    /// downloaded at most once for a version, and never again.
    async fn metadata(
        &self,
        name: &pep508_rs::PackageName,
        version: &pep440_rs::Version,
        candidate: &Candidate,
    ) -> Result<WheelMetadata, String> {
        let canonical_name = canonical_project_name(name)?;
        let wheel_url = url::Url::parse(&candidate.wheel.url)
            .map_err(|err| format!("parse the wheel URL for {name}: {err}"))?;
        validate_url(&wheel_url).map_err(|err| super::report_message(&err))?;
        let auth = self.auth_for(&canonical_name);
        // An index that publishes a metadata file vouches for a digest
        // `cached_metadata` re-checks. One that publishes none leaves the
        // extracted document nothing to be checked against, so its cache
        // entry is bound to the wheel it came out of instead.
        let derived_from = match candidate.core_metadata {
            Some(_) => None,
            None => candidate.wheel.hashes.get("sha256").map(String::as_str),
        };
        let cache_path = self.cache_path(&auth, &metadata_url(&wheel_url), derived_from);
        if let Some(cached) = self.cached(&cache_path).await {
            let document = self.hold("metadata", cached.body)?;
            return Self::cached_metadata(&document, name, version, candidate);
        }
        let _reading = self.locks.lock(&cache_path.to_string_lossy()).await;
        if let Some(cached) = self.cached(&cache_path).await {
            let document = self.hold("metadata", cached.body)?;
            return Self::cached_metadata(&document, name, version, candidate);
        }
        let document = if let Some(digests) = &candidate.core_metadata {
            let (document, _) = self
                .fetch(&auth, &metadata_url(&wheel_url), "metadata", MAX_METADATA_BYTES, None)
                .await?;
            verify_digest(&document, digests, "metadata file", &candidate.wheel.name)?;
            document
        } else {
            let (wheel, _) = self.fetch(&auth, &wheel_url, "wheel", MAX_WHEEL_BYTES, None).await?;
            verify_digest(&wheel, &candidate.wheel.hashes, "wheel", &candidate.wheel.name)?;
            metadata_from_wheel(&wheel, &candidate.wheel.name)?
        };
        let document = text(document, "metadata", &candidate.wheel.name)?;
        let metadata = parse_metadata(&document, name, version, &candidate.wheel.name)?;
        Self::store(cache_path, CachedDocument { url: wheel_url.to_string(), body: document })
            .await;
        Ok(metadata)
    }

    /// A cached metadata document, checked against what the index says
    /// about it *now*: the digests come from the project page this resolve
    /// just read, so a file republished with different content is not
    /// answered from what was cached under the old one.
    fn cached_metadata(
        document: &str,
        name: &pep508_rs::PackageName,
        version: &pep440_rs::Version,
        candidate: &Candidate,
    ) -> Result<WheelMetadata, String> {
        if let Some(digests) = &candidate.core_metadata {
            verify_digest(document.as_bytes(), digests, "metadata file", &candidate.wheel.name)?;
        }
        parse_metadata(document, name, version, &candidate.wheel.name)
    }

    /// Read a document from the index, against this resolve's budget and
    /// this deployment's route policy. Returns the bytes and the URL they
    /// were read from, which relative links resolve against.
    async fn fetch(
        &self,
        auth: &AuthHeaders,
        url: &url::Url,
        kind: &str,
        limit: usize,
        accept: Option<&str>,
    ) -> Result<(Vec<u8>, url::Url), String> {
        if !within_budget(self.bytes_held.load(Ordering::Relaxed)) {
            return Err(budget_exhausted(kind));
        }
        // The route policy decides what this deployment may reach at all;
        // an index a caller merely names is refused here rather than
        // fetched (SSRF boundary).
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
                auth,
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
        Ok((self.hold(kind, response.body)?, source))
    }

    /// Account bytes against this resolve's budget, which bounds what one
    /// request can make the server hold and cache.
    fn hold<Body: AsRef<[u8]>>(&self, kind: &str, body: Body) -> Result<Body, String> {
        let held =
            self.bytes_held.fetch_add(body.as_ref().len(), Ordering::Relaxed) + body.as_ref().len();
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
    /// Where the document read from `url` is cached. `derived_from` is the
    /// digest of the artifact a document was extracted from rather than
    /// read whole, and joins the key so a republished artifact is read
    /// again rather than answered from what came out of the old one.
    fn cache_path(
        &self,
        auth: &AuthHeaders,
        url: &url::Url,
        derived_from: Option<&str>,
    ) -> PathBuf {
        let scope = match auth.metadata_scope(url.as_str(), None) {
            MetadataCacheScope::Public => "public".to_string(),
            MetadataCacheScope::Private { descriptor_id } => descriptor_id,
        };
        let key = match derived_from {
            Some(digest) => format!("{url}#{digest}"),
            None => url.to_string(),
        };
        self.cache_dir.join(scope).join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(&key)))
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

/// The candidates a project page offers, refusing a page that is not one.
fn parse_page(
    page: &str,
    page_url: &url::Url,
    name: &pep508_rs::PackageName,
    target: &Target,
) -> Result<BTreeMap<pep440_rs::Version, Candidate>, String> {
    candidates_from_page(page, page_url, name, target).map_err(|err| super::report_message(&err))
}

/// A document as text, refusing one that is not.
fn text(bytes: Vec<u8>, kind: &str, of: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|err| format!("decode the {kind} of {of}: {err}"))
}

/// The metadata a document describes, refusing one that describes another
/// distribution: what a wheel requires decides what a client installs, so
/// metadata for something else must not stand in for it.
fn parse_metadata(
    document: &str,
    name: &pep508_rs::PackageName,
    version: &pep440_rs::Version,
    filename: &str,
) -> Result<WheelMetadata, String> {
    let metadata = WheelMetadata::parse(document).map_err(|err| super::report_message(&err))?;
    let named = metadata
        .name
        .parse::<pep508_rs::PackageName>()
        .map_err(|err| format!("read the distribution the metadata of {filename} names: {err}"))?;
    let versioned = metadata
        .version
        .parse::<pep440_rs::Version>()
        .map_err(|err| format!("read the version the metadata of {filename} names: {err}"))?;
    if named != *name || versioned != *version {
        return Err(format!(
            "the metadata of {filename} describes {named} {versioned}, not {name} {version}",
        ));
    }
    Ok(metadata)
}

/// The project page of `canonical_name` under an index, keeping whatever
/// query the index URL carries: an index can put a token there, and a page
/// addressed without it is a different request.
fn project_page_url(index: &url::Url, canonical_name: &str) -> Result<url::Url, String> {
    let mut url = index.clone();
    url.set_path(&format!("{}{canonical_name}/", index.path()));
    Ok(url)
}

/// The metadata file published beside a wheel (PEP 658), which is the
/// wheel's own address with `.metadata` on the end of its path — the query
/// stays where it is.
fn metadata_url(wheel: &url::Url) -> url::Url {
    let mut url = wheel.clone();
    url.set_path(&format!("{}.metadata", wheel.path()));
    url
}

/// A document as it was read, beside the URL it came from: a redirected
/// page's links resolve against where it landed, not where it was asked
/// for.
///
/// The body is text, which is what both cached documents are — a Simple
/// API page and a `METADATA` file — and what keeps the cache entry the
/// size of the document rather than the decimal array JSON would make of
/// its bytes.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedDocument {
    url: String,
    body: String,
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
fn metadata_from_wheel(wheel: &[u8], filename: &str) -> Result<Vec<u8>, String> {
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
    let mut document = Vec::new();
    archive
        .by_name(&entry)
        .map_err(|err| format!("read {entry} from {filename}: {err}"))?
        // One byte past the cap, so a document that reaches it is refused
        // rather than read as a whole one: a `METADATA` cut short still
        // names its distribution, and the requirements after the cut would
        // silently not exist.
        .take(MAX_METADATA_BYTES as u64 + 1)
        .read_to_end(&mut document)
        .map_err(|err| format!("read {entry} from {filename}: {err}"))?;
    if document.len() > MAX_METADATA_BYTES {
        return Err(format!("the metadata in {filename} exceeds {MAX_METADATA_BYTES} bytes"));
    }
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
    pnpr_package_name::CanonicalPackageName::parse(name.as_ref(), pnpr_registry::Ecosystem::Pypi)
        .map(|name| name.as_str().to_string())
        .map_err(|err| err.to_string())
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
