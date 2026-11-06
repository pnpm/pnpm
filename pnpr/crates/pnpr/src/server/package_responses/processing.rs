use super::{
    super::{
        describe_tars, filter_osv_vulnerable_versions, rewrite_tarball_urls,
        rewrite_upstream_tarball_urls,
    },
    AppState, CanonicalPackageName, DistView, HeaderMap, RegistryError, Representation, Value,
};
use axum::body::Bytes;
use chrono::{DateTime, Utc};
use pnpr_storage::BoundedCache;
use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    sync::{Arc, Mutex},
};

/// Source and rendered response bytes retained by one server instance.
const CACHE_BUDGET: usize = 128 * 1024 * 1024;

pub(crate) struct PackumentProcessing {
    workers: Arc<tokio::sync::Semaphore>,
    cache: Mutex<BoundedCache<CacheKey, Arc<RenderedPackument>>>,
    fingerprints: RandomState,
}

impl Default for PackumentProcessing {
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(1, usize::from).min(8);
        Self {
            // Scale CPU work with the instance while bounding document expansion.
            workers: Arc::new(tokio::sync::Semaphore::new(workers)),
            cache: Mutex::new(BoundedCache::default()),
            fingerprints: RandomState::new(),
        }
    }
}

impl PackumentProcessing {
    /// The permit stays with the blocking work even if its request is cancelled.
    pub(crate) async fn run<Output, Work>(&self, work: Work) -> Result<Output, RegistryError>
    where
        Output: Send + 'static,
        Work: FnOnce() -> Output + Send + 'static,
    {
        let workers = Arc::clone(&self.workers);
        let permit = workers.acquire_owned().await.expect("packument workers never close");
        Ok(tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await?)
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    fingerprint: u64,
    context: String,
}

pub(super) struct RenderedPackument {
    source: Bytes,
    pub(super) body: Bytes,
    pub(super) content_type: &'static str,
    pub(super) etag: String,
    pub(super) last_modified: Option<String>,
    rendered_at: DateTime<Utc>,
    expires: Option<DateTime<Utc>>,
}

pub(super) async fn render(
    name: &CanonicalPackageName,
    bytes: Bytes,
    tarball_base: &str,
    view: DistView<'_>,
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Arc<RenderedPackument>, RegistryError> {
    let name = name.clone();
    let tarball_base = tarball_base.to_string();
    let revision_registry = view.revision_registry.map(str::to_string);
    let tar_integrities = view.tar_integrities.cloned();
    let tar_registry = view.tar_registry;
    let representation = Representation::requested(headers);
    let owned_state = state.clone();
    state.inner.packuments.run(move || {
        let view = DistView {
            revision_registry: revision_registry.as_deref(),
            tar_integrities: tar_integrities.as_ref(),
            tar_registry,
        };
        render_cached(&name, bytes, &tarball_base, view, &owned_state, representation)
    })
    .await?
}

fn render_cached(
    name: &CanonicalPackageName,
    bytes: Bytes,
    tarball_base: &str,
    view: DistView<'_>,
    state: &AppState,
    representation: Representation,
) -> Result<Arc<RenderedPackument>, RegistryError> {
    let key = cache_key(name, &bytes, tarball_base, view, representation, &state.inner.packuments)?;
    let cache = state.inner.packuments.cache.lock().expect("packument cache poisoned");
    let now = Utc::now();
    if let Some(rendered) = cache.get(&key)
        && rendered.source == bytes
        && now >= rendered.rendered_at
        && rendered.expires.is_none_or(|expires| now <= expires)
    {
        return Ok(Arc::clone(rendered));
    }
    drop(cache);
    let rendered =
        Arc::new(render_document(name, bytes, tarball_base, view, state, representation, now)?);
    let weight = rendered.source.len() + rendered.body.len() + key.context.len() + 256;
    state.inner.packuments.cache
        .lock()
        .expect("packument cache poisoned")
        .insert(key, Arc::clone(&rendered), weight, CACHE_BUDGET);
    Ok(rendered)
}

fn cache_key(
    name: &CanonicalPackageName,
    bytes: &[u8],
    tarball_base: &str,
    view: DistView<'_>,
    representation: Representation,
    processing: &PackumentProcessing,
) -> Result<CacheKey, RegistryError> {
    let context = serde_json::to_vec(&(
        name.as_str(),
        tarball_base,
        view.revision_registry,
        view.tar_integrities.map(AsRef::as_ref),
        view.tar_registry,
        view.hosted,
        representation as u8,
    ))?;
    // Fingerprints only select candidates. Reuse requires matching source bytes.
    Ok(CacheKey {
        fingerprint: processing.fingerprints.hash_one(bytes),
        context: super::super::ecosystem::sha256_hex(&context),
    })
}

fn render_document(
    name: &CanonicalPackageName,
    bytes: Bytes,
    tarball_base: &str,
    view: DistView<'_>,
    state: &AppState,
    representation: Representation,
    now: DateTime<Utc>,
) -> Result<RenderedPackument, RegistryError> {
    let mut doc: Value = serde_json::from_slice(&bytes)?;
    if view.hosted && representation == Representation::Full {
        doc["_rev"] = Value::String(super::super::package_mutation::revisions::revision(&doc));
    }
    let osv_index = state.inner.osv_index.as_ref();
    filter_osv_vulnerable_versions(&mut doc, name, osv_index);
    match view.revision_registry {
        Some(source) => rewrite_upstream_tarball_urls(&mut doc, name, source, tarball_base),
        None => rewrite_tarball_urls(&mut doc, name, tarball_base),
    }
    if let Some(table) = view.tar_integrities {
        describe_tars(&mut doc, table, view.tar_registry);
    }
    let last_modified = if osv_index.is_some() || representation != Representation::Full {
        None
    } else {
        super::packument_last_modified(&doc)
    };
    let expires = cache_expiry(representation, &doc, now);
    let (body, content_type) =
        representation.render(doc, &state.inner.config.http.package_documents, now)?;
    let etag = format!(r#""{}""#, super::super::ecosystem::sha256_hex(&body));
    Ok(RenderedPackument {
        source: bytes,
        body: body.into(),
        content_type,
        etag,
        last_modified,
        rendered_at: now,
        expires,
    })
}

fn cache_expiry(
    representation: Representation,
    doc: &Value,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match representation {
        Representation::Abbreviated => next_precision_change(doc, now),
        Representation::InstallV2
            if super::super::split_packuments::document_anchor(doc).is_none() =>
        {
            next_precision_change(doc, now)
        }
        Representation::Full | Representation::InstallV2 => None,
    }
}

fn next_precision_change(doc: &Value, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    doc.get("time")?
        .as_object()?
        .values()
        .filter_map(Value::as_str)
        .filter_map(|value| DateTime::parse_from_rfc3339(value).ok())
        .filter_map(|time| {
            time.with_timezone(&Utc)
                .checked_add_signed(chrono::Duration::days(
                    pnpr_upstream::TIME_PRECISION_HORIZON_DAYS,
                ))
        })
        .filter(|time| *time >= now)
        .min()
}

#[cfg(test)]
mod tests;
