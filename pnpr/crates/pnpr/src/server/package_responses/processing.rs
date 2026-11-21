pub(crate) mod admission;

use super::{
    super::{
        describe_tars,
        filter_osv_vulnerable_versions,
        rewrite_tarball_urls,
        rewrite_upstream_tarball_urls,
    },
    AppState,
    CanonicalPackageName,
    DistView,
    HeaderMap,
    RegistryError,
    Representation,
    Value,
};
use axum::body::Bytes;
use jiff::Timestamp;
use pnpr_storage::BoundedCache;
use std::sync::{
    Arc,
    Mutex,
};

/// Rendered response bytes retained by one server instance.
const CACHE_BUDGET: usize = 128 * 1024 * 1024;

mod contexts;
mod workers;
use admission::Admissions;
use contexts::TarContexts;
use workers::Workers;

type RenderFlight =
    super::super::Singleflight<CacheKey, Result<Arc<RenderedPackument>, Arc<RegistryError>>>;

pub(crate) struct PackumentProcessing {
    pub(crate) workers: Workers,
    lookups: Workers,
    pub(crate) admissions: Admissions,
    cache: Mutex<BoundedCache<CacheKey, Arc<RenderedPackument>>>,
    cache_budget: usize,
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
            cache_budget: CACHE_BUDGET,
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

// The cache holds no source document, so the digests are all that tie a
// response to the bytes it was rendered from.
#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    source: String,
    context: String,
}

pub(super) struct RenderedPackument {
    pub(super) body: Bytes,
    pub(super) content_type: &'static str,
    pub(super) etag: String,
    pub(super) last_modified: Option<String>,
    rendered_at: Timestamp,
    expires: Option<Timestamp>,
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
        let cached = cached_document(&key, &lookup_state.inner.packuments);
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
    state.inner.packuments.in_flight
        .work(key.clone(), || async { render_job(input, key, state).await.map_err(Arc::new) })
        .await
        .map_err(RegistryError::Shared)
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
    processing: &PackumentProcessing,
) -> Option<Arc<RenderedPackument>> {
    let cached = processing.cache
        .lock()
        .expect("packument cache poisoned")
        .get(key)
        .cloned();
    let now = Timestamp::now();
    cached.filter(|rendered| {
        now >= rendered.rendered_at && rendered.expires.is_none_or(|expires| now <= expires)
    })
}

fn render_miss(
    input: &RenderInput,
    key: CacheKey,
    state: &AppState,
) -> Result<Arc<RenderedPackument>, RegistryError> {
    if let Some(cached) = cached_document(&key, &state.inner.packuments) {
        return Ok(cached);
    }
    let rendered = Arc::new(render_document(
        &input.name,
        &input.bytes,
        &input.tarball_base,
        input.view.view(),
        state,
        input.representation,
        Timestamp::now(),
    )?);
    let weight = rendered.body.len() + key.source.len() + key.context.len() + 256;
    state.inner.packuments.cache
        .lock()
        .expect("packument cache poisoned")
        .insert(key, Arc::clone(&rendered), weight, state.inner.packuments.cache_budget);
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
    Ok(CacheKey {
        source: super::super::ecosystem::sha256_hex(bytes),
        context: super::super::ecosystem::sha256_hex(&context),
    })
}

fn render_document(
    name: &CanonicalPackageName,
    bytes: &[u8],
    tarball_base: &str,
    view: DistView<'_>,
    state: &AppState,
    representation: Representation,
    now: Timestamp,
) -> Result<RenderedPackument, RegistryError> {
    let mut doc: Value = serde_json::from_slice(bytes)?;
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
        representation.render(doc, &state.inner.config.http.package_documents, now, view)?;
    let etag = format!(r#""{}""#, super::super::ecosystem::sha256_hex(&body));
    Ok(RenderedPackument {
        body: body.into(),
        content_type,
        etag,
        last_modified,
        rendered_at: now,
        expires,
    })
}

fn cache_expiry(representation: Representation, doc: &Value, now: Timestamp) -> Option<Timestamp> {
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

fn next_precision_change(doc: &Value, now: Timestamp) -> Option<Timestamp> {
    doc.get("time")?
        .as_object()?
        .values()
        .filter_map(Value::as_str)
        .map(pnpm_time::parse_rfc3339)
        .filter_map(Result::ok)
        .filter_map(|time| {
            time.checked_add(jiff::SignedDuration::from_hours(
                24 * pnpr_upstream::TIME_PRECISION_HORIZON_DAYS,
            ))
            .ok()
        })
        .filter(|time| *time >= now)
        .min()
}

#[cfg(test)]
mod tests;
