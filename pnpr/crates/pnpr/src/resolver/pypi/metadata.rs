use super::{
    BTreeMap, Candidate, Cursor, MAX_METADATA_BYTES, Target, WheelMetadata, candidates_from_page,
};
use std::io::Read;

/// The candidates a project page offers, refusing a page that is not one.
pub(super) fn parse_page(
    page: &str,
    page_url: &url::Url,
    name: &pep508_rs::PackageName,
    target: &Target,
) -> Result<BTreeMap<pep440_rs::Version, Candidate>, String> {
    candidates_from_page(page, page_url, name, target)
        .map_err(|err| super::super::report_message(&err))
}

/// A document as text, refusing one that is not.
pub(super) fn text(bytes: Vec<u8>, kind: &str, of: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|err| format!("decode the {kind} of {of}: {err}"))
}

/// The metadata a document describes, refusing one that describes another
/// distribution: what a wheel requires decides what a client installs, so
/// metadata for something else must not stand in for it.
pub(super) fn parse_metadata(
    document: &str,
    name: &pep508_rs::PackageName,
    version: &pep440_rs::Version,
    filename: &str,
) -> Result<WheelMetadata, String> {
    let metadata =
        WheelMetadata::parse(document).map_err(|err| super::super::report_message(&err))?;
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
pub(super) fn project_page_url(index: &url::Url, canonical_name: &str) -> Result<url::Url, String> {
    let mut url = index.clone();
    url.set_path(&format!("{}{canonical_name}/", index.path()));
    Ok(url)
}

/// The metadata file published beside a wheel (PEP 658), which is the
/// wheel's own address with `.metadata` on the end of its path — the query
/// stays where it is.
pub(super) fn metadata_url(wheel: &url::Url) -> url::Url {
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
pub(super) fn metadata_from_wheel(wheel: &[u8], filename: &str) -> Result<Vec<u8>, String> {
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
    let metadata =
        archive.by_name(&entry).map_err(|err| format!("read {entry} from {filename}: {err}"))?;
    // One byte past the cap, so a document that reaches it is refused
    // rather than read as a whole one: a `METADATA` cut short still
    // names its distribution, and the requirements after the cut would
    // silently not exist.
    let mut capped = metadata.take(MAX_METADATA_BYTES as u64 + 1);
    capped
        .read_to_end(&mut document)
        .map_err(|err| format!("read {entry} from {filename}: {err}"))?;
    if document.len() > MAX_METADATA_BYTES {
        return Err(format!("the metadata in {filename} exceeds {MAX_METADATA_BYTES} bytes"));
    }
    Ok(document)
}
