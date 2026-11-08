use super::{BTreeMap, Offered, Target, WheelMetadata, candidates_from_page};
use pnpm_python_resolver::matches_published_digest;

/// What a project page offers, refusing a page that is not one. The
/// releases it leaves out travel with the candidates: they are what a
/// resolution failure tells the client about a distribution that offered
/// it nothing.
pub(super) fn parse_page(
    page: &str,
    page_url: &url::Url,
    name: &pep508_rs::PackageName,
    target: &Target,
) -> Result<Offered, String> {
    candidates_from_page(page, page_url, name, target)
        .map_err(|err| super::super::report_message(&err))
}

/// A document as text, refusing one that is not.
pub(super) fn text(bytes: Vec<u8>, kind: &str, of: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|err| format!("decode the {kind} of {of}: {err}"))
}

/// The metadata a document describes, refusing one that describes another
/// distribution.
pub(super) fn parse_metadata(
    document: &str,
    name: &pep508_rs::PackageName,
    version: &pep440_rs::Version,
    filename: &str,
) -> Result<WheelMetadata, String> {
    WheelMetadata::parse_for(document, name, version, filename)
        .map_err(|err| super::super::report_message(&err))
}

/// The project page of `canonical_name` under an index, keeping whatever
/// query the index URL carries: an index can put a token there, and a page
/// addressed without it is a different request.
pub(super) fn project_page_url(index: &url::Url, canonical_name: &str) -> Result<url::Url, String> {
    let mut url = index.clone();
    url.set_path(&format!("{}{canonical_name}/", index.path()));
    Ok(url)
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
pub(super) struct CachedDocument {
    pub(super) url: String,
    pub(super) body: String,
}

impl CachedDocument {
    pub(super) fn url(&self, requested: &url::Url) -> Result<url::Url, String> {
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
pub(super) fn verify_digest(
    bytes: &[u8],
    digests: &BTreeMap<String, String>,
    kind: &str,
    filename: &str,
) -> Result<(), String> {
    if !matches_published_digest(bytes, digests) {
        return Err(format!(
            "the {kind} of {filename} does not match the SHA-256 the index published",
        ));
    }
    Ok(())
}

/// The `METADATA` inside a wheel, for an index that publishes no metadata
/// file of its own.
pub(super) fn metadata_from_wheel(wheel: &[u8], filename: &str) -> Result<Vec<u8>, String> {
    pnpr_pypi::archive::metadata_from_zip(wheel, filename, pnpr_pypi::DistributionKind::Wheel)
}
