pub(crate) mod admission;

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

mod contexts;
mod workers;
use admission::Admissions;
use contexts::TarContexts;
use workers::Workers;

type RenderFlight = super::super::Singleflight<CacheKey, SharedRender>;

/// Retains the source on failure so fingerprint collisions cannot share errors.
#[derive(Clone)]
struct SharedRender {
    source: Bytes,
    result: Result<Arc<RenderedPackument>, Arc<RegistryError>>,
}

pub(crate) struct PackumentProcessing {
    pub(crate) workers: Workers,
    lookups: Workers,
    pub(crate) admissions: Admissions,
    cache: Mutex<BoundedCache<CacheKey, Arc<RenderedPackument>>>,
    fingerprints: RandomState,
    contexts: TarContexts,
    in_flight: RenderFlight,
}

impl Default for PackumentProcessing {
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(1, usize::from).min(8);
        Self {
            workers: Workers::new(workers),
            lookups: Workers::new(workers.min(2)),
            // Source slots cover I/O as well as CPU work.
            admissions: Admissions::new(workers * 8),
            cache: Mutex::new(BoundedCache::default()),
            fingerprints: RandomState::new(),
            contexts: TarContexts::default(),
            in_flight: RenderFlight::new(),
        }
    }
}

#[derive(Clone)]
struct RenderInput {
    name: CanonicalPackageName,
    bytes: Bytes,
    tarball_base: String,
    view: OwnedView,
    representation: Representation,
}

#[derive(Clone)]
struct OwnedView {
    revision_registry: Option<String>,
    tar_integrities: Option<Arc<std::collections::BTreeMap<String, pnpr_storage::DerivedTar>>>,
    tar_registry: bool,
    hosted: bool,
}

impl OwnedView {
    fn view(&self) -> DistView<'_> {
        DistView {
            revision_registry: self.revision_registry.as_deref(),
            tar_integrities: self.tar_integrities.as_ref(),
            tar_registry: self.tar_registry,
            hosted: self.hosted,
        }
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
    let input = RenderInput {
        name: name.clone(),
        bytes,
        tarball_base: tarball_base.to_string(),
        view: OwnedView {
            revision_registry: view.revision_registry.map(str::to_string),
            tar_integrities: view.tar_integrities.cloned(),
            tar_registry: view.tar_registry,
            hosted: view.hosted,
        },
        representation: Representation::requested(headers),
    };
    let lookup_input = input.clone();
    let lookup_state = state.clone();
    let (key, cached) = state.inner.packuments.lookups.run(move || {
        let key = input_key(&lookup_input, &lookup_state.inner.packuments)?;
        let cached = cached_document(&key, &lookup_input.bytes, &lookup_state.inner.packuments);
        Ok::<_, RegistryError>((key, cached))
    })
    .await??;
    if let Some(cached) = cached {
        return Ok(cached);
    }
    render_shared(input, key, state).await
}

async fn render_shared(
    input: RenderInput,
    key: CacheKey,
    state: &AppState,
) -> Result<Arc<RenderedPackument>, RegistryError> {
    let leader_input = input.clone();
    let leader_key = key.clone();
    let shared = state.inner.packuments.in_flight.work(key.clone(), || async {
        let source = leader_input.bytes.clone();
        let result = render_job(leader_input, leader_key, state).await.map_err(Arc::new);
        SharedRender { source, result }
    })
    .await;
    let source = input.bytes.clone();
    let candidate = shared.source.clone();
    let matches = state.inner.packuments.lookups.run(move || candidate == source).await?;
    if matches {
        return shared.result.map_err(RegistryError::Shared);
    }
    render_job(input, key, state).await
}

async fn render_job(
    input: RenderInput,
    key: CacheKey,
    state: &AppState,
) -> Result<Arc<RenderedPackument>, RegistryError> {
    let owned_state = state.clone();
    state.inner.packuments.workers.run(move || render_miss(&input, key, &owned_state)).await?
}

fn input_key(
    input: &RenderInput,
    processing: &PackumentProcessing,
) -> Result<CacheKey, RegistryError> {
    cache_key(
        &input.name,
        &input.bytes,
        &input.tarball_base,
        input.view.view(),
        input.representation,
        processing,
    )
}

fn cached_document(
    key: &CacheKey,
    bytes: &Bytes,
    processing: &PackumentProcessing,
) -> Option<Arc<RenderedPackument>> {
    let cached = processing.cache
        .lock()
        .expect("packument cache poisoned")
        .get(key)
        .cloned();
    let now = Utc::now();
    cached.filter(|rendered| {
        rendered.source == *bytes
            && now >= rendered.rendered_at
            && rendered.expires.is_none_or(|expires| now <= expires)
    })
}

fn render_miss(
    input: &RenderInput,
    key: CacheKey,
    state: &AppState,
) -> Result<Arc<RenderedPackument>, RegistryError> {
    if let Some(cached) = cached_document(&key, &input.bytes, &state.inner.packuments) {
        return Ok(cached);
    }
    let rendered = Arc::new(render_document(
        &input.name,
        input.bytes.clone(),
        &input.tarball_base,
        input.view.view(),
        state,
        input.representation,
        Utc::now(),
    )?);
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
    let tar_context = view.tar_integrities
        .map(|table| processing.contexts.digest(table))
        .transpose()?;
    let context = serde_json::to_vec(&(
        name.as_str(),
        tarball_base,
        view.revision_registry,
        tar_context.as_deref(),
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
