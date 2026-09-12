use super::{
    Action, AppState, Body, Ecosystem, FetchOutcome, HashSet, HostedGate, Identity, Registry,
    RegistryError, RegistrySource, Response, StatusCode, Storage, Upstream, Value, authorize,
    default_registry_target, header, hosted_gate, hosted_storage, json, now_iso,
    resolve_ecosystem_source, resolve_registry_source,
};
use axum::response::IntoResponse;

pub(super) enum DiscoverySource {
    Hosted(String),
    Upstream(String),
}

/// One page of search results, in the order the discovery sources are
/// walked. `names` counts every match so a caller can report a total that
/// is larger than the page, and it is what makes the first source to serve
/// a name the one that owns it. `Item` is whatever the surface renders.
pub(super) struct SearchPage<Item> {
    pub(super) objects: Vec<Item>,
    pub(super) names: HashSet<String>,
    pub(super) from: usize,
    pub(super) size: usize,
}

impl<Item> SearchPage<Item> {
    pub(super) fn new(from: usize, size: usize) -> Self {
        Self { objects: Vec::new(), names: HashSet::new(), from, size }
    }

    pub(super) fn push_name(&mut self, name: &str) -> bool {
        let position = self.names.len();
        if self.names.insert(name.to_string())
            && position >= self.from
            && position < self.from.saturating_add(self.size)
        {
            return true;
        }
        false
    }

    pub(super) fn total(&self) -> usize {
        self.names.len()
    }
}

impl SearchPage<Value> {
    pub(super) fn push(&mut self, object: Value) {
        let Some(name) = search_object_name(&object) else {
            return;
        };
        if self.push_name(name) {
            self.objects.push(object);
        }
    }
}

pub(super) const MAX_UPSTREAM_SEARCH_RESULTS: usize = 2_000;

pub(super) const MAX_UPSTREAM_SEARCH_PAGES: usize = 8;

#[derive(Default)]
pub(super) struct UpstreamSearchBudget {
    pub(super) pages: usize,
    pub(super) results: usize,
}

pub(super) struct UpstreamSearchContext<'a> {
    pub(super) state: &'a AppState,
    pub(super) identity: &'a Identity,
    pub(super) registry: &'a str,
    pub(super) source: &'a str,
    pub(super) upstream: &'a Upstream,
    pub(super) query_string: &'a str,
}

impl UpstreamSearchBudget {
    pub(super) fn remaining_results(&self) -> usize {
        MAX_UPSTREAM_SEARCH_RESULTS.saturating_sub(self.results)
    }

    /// Charge one upstream page against the budget.
    pub(super) fn take_page(&mut self) -> Result<(), RegistryError> {
        if self.pages == MAX_UPSTREAM_SEARCH_PAGES {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "upstream search is limited to {MAX_UPSTREAM_SEARCH_PAGES} pages; refine the query",
                ),
            });
        }
        self.pages += 1;
        Ok(())
    }

    /// Charge one page's results against the budget, refusing a source that
    /// reports more than the whole search may scan.
    pub(super) fn take_results(
        &mut self,
        reported_total: usize,
        object_count: usize,
        source_budget: usize,
    ) -> Result<(), RegistryError> {
        if reported_total > source_budget || object_count > self.remaining_results() {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "upstream search is limited to {MAX_UPSTREAM_SEARCH_RESULTS} results; refine the query",
                ),
            });
        }
        self.results += object_count;
        Ok(())
    }
}

/// `GET /-/v1/search?text=...&from=...&size=...` — npm search v1 endpoint.
/// Hosted results are counted after routing and access filters, then optional
/// upstream results are appended in registry-source order. Every participating
/// upstream is exhausted so `total` describes the complete visible,
/// deduplicated result set. An upstream only participates when its `search`
/// setting is enabled.
pub(super) async fn serve_search(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    query_string: &str,
) -> Response {
    let Some(params) = pnpr_search::parse_params(query_string, 20) else {
        return search_response(&[], 0);
    };
    let Some(registry) =
        registry.map(str::to_string).or_else(|| default_registry_target(state, Ecosystem::Npm))
    else {
        return search_response(&[], 0);
    };
    let browse = pnpr_search::browse_requested(query_string);
    let mut page = SearchPage::new(params.from, params.size);
    let mut upstream_budget = UpstreamSearchBudget::default();
    for source in discovery_sources(state, &registry, Ecosystem::Npm) {
        let searched = match source {
            DiscoverySource::Hosted(source) => {
                append_hosted_search(
                    state,
                    identity,
                    HostedSearch { registry: &registry, source: &source, text: &params.text },
                    &mut page,
                )
                .await
            }
            DiscoverySource::Upstream(source) => {
                let search = UpstreamSearch {
                    registry: &registry,
                    source: &source,
                    query_string,
                    browse,
                    from: params.from,
                };
                append_upstream_source(state, identity, search, &mut page, &mut upstream_budget)
                    .await
            }
        };
        if let Err(err) = searched {
            return err.into_response();
        }
    }
    let total = page.total();
    search_response(&page.objects, total)
}

pub(super) fn search_response(objects: &[Value], total: usize) -> Response {
    let body = json!({ "objects": objects, "total": total, "time": now_iso() });
    let bytes = serde_json::to_vec(&body).expect("search response serializes");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// One hosted source of a search.
pub(super) struct HostedSearch<'a> {
    pub(super) registry: &'a str,
    pub(super) source: &'a str,
    pub(super) text: &'a pnpr_search::SearchText,
}

/// Add one hosted source's matches to the page.
pub(super) async fn append_hosted_search(
    state: &AppState,
    identity: &Identity,
    search: HostedSearch<'_>,
    page: &mut SearchPage<Value>,
) -> Result<(), RegistryError> {
    let hosted = hosted_search_names(
        state,
        identity,
        search.registry,
        search.source,
        Ecosystem::Npm,
        search.text,
    )
    .await?;
    let Some((storage, names)) = hosted else {
        return Ok(());
    };
    for name in names {
        if page.push_name(&name) {
            page.objects.push(pnpr_search::local_search_entry(&storage, &name).await);
        }
    }
    Ok(())
}

/// One upstream source of a search.
pub(super) struct UpstreamSearch<'a> {
    pub(super) registry: &'a str,
    pub(super) source: &'a str,
    pub(super) query_string: &'a str,
    /// A browse request lists what pnpr itself holds, so no upstream is asked.
    pub(super) browse: bool,
    pub(super) from: usize,
}

/// Add one upstream source's matches to the page, if the caller may reach it
/// and the budget allows.
pub(super) async fn append_upstream_source(
    state: &AppState,
    identity: &Identity,
    search: UpstreamSearch<'_>,
    page: &mut SearchPage<Value>,
    budget: &mut UpstreamSearchBudget,
) -> Result<(), RegistryError> {
    let Some(config) = state.inner.config.upstreams.get(search.source) else {
        return Ok(());
    };
    if search.browse || !config.search || !upstream_search_admits(config, identity) {
        return Ok(());
    }
    let Some(upstream) = state.inner.upstreams.get(search.source) else {
        return Ok(());
    };
    if search.from > page.total().saturating_add(budget.remaining_results()) {
        return Err(RegistryError::BadRequest {
            reason: format!(
                "search `from` would require scanning more than {MAX_UPSTREAM_SEARCH_RESULTS} upstream results",
            ),
        });
    }
    let context = UpstreamSearchContext {
        state,
        identity,
        registry: search.registry,
        source: search.source,
        upstream,
        query_string: search.query_string,
    };
    append_upstream_search(context, page, budget).await
}

/// Whether an upstream's own gates admit this caller to its search.
pub(super) fn upstream_search_admits(
    config: &pnpr_config::UpstreamConfig,
    identity: &Identity,
) -> bool {
    config.access.as_ref().is_none_or(|access| access.allows(identity))
        && config.rules.all_access_admit(identity)
}

/// The hosted names one discovery source contributes to a search, beside the
/// storage they were read from. `None` when the caller may not see the source
/// at all. Only the projection of a name into a result differs between
/// ecosystems, so the walk itself lives here.
pub(super) async fn hosted_search_names(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    source: &str,
    ecosystem: Ecosystem,
    text: &pnpr_search::SearchText,
) -> Result<Option<(Storage, Vec<String>)>, RegistryError> {
    let Some(hosted) = state.inner.config.hosted.get(source) else {
        return Ok(None);
    };
    if !hosted.rules.any_access_admits(identity) {
        return Ok(None);
    }
    let storage = hosted_storage(state, Some(&hosted.org));
    let keep = |name: &str| {
        matches!(
            resolve_ecosystem_source(state, registry, ecosystem, name),
            RegistrySource::Hosted(resolved) if resolved == source,
        ) && matches!(hosted_gate(state, identity, source, name), HostedGate::Allowed(_))
    };
    let names = pnpr_search::local_search_names(&storage, text, keep).await?;
    Ok(Some((storage, names)))
}

pub(super) async fn append_upstream_search(
    context: UpstreamSearchContext<'_>,
    page: &mut SearchPage<Value>,
    budget: &mut UpstreamSearchBudget,
) -> Result<(), RegistryError> {
    const FETCH_SIZE: usize = 250;

    let resolved = RegistrySource::Upstream(context.source.to_string());
    let source_result_budget = budget.remaining_results();
    let mut from = 0usize;
    loop {
        budget.take_page()?;
        let query = upstream_search_query(context.query_string, from, FETCH_SIZE);
        let response = match context.upstream.fetch_search(&query).await? {
            FetchOutcome::Ok(response) => response,
            FetchOutcome::NotFound => return Ok(()),
        };
        let object_count = response.objects.len();
        budget.take_results(response.total, object_count, source_result_budget)?;
        append_visible_results(&context, &resolved, response.objects, page);
        from = from.saturating_add(object_count);
        if from >= response.total {
            return Ok(());
        }
        if object_count == 0 {
            return Err(RegistryError::UpstreamResponse {
                url: format!("{}/-/v1/search", context.source),
                reason: format!("reported {} results but returned an empty page", response.total),
            });
        }
    }
}

/// Take the results of one upstream page that this caller may see.
pub(super) fn append_visible_results(
    context: &UpstreamSearchContext<'_>,
    resolved: &RegistrySource,
    objects: Vec<Value>,
    page: &mut SearchPage<Value>,
) {
    for object in objects {
        if search_result_is_visible(context, resolved, &object) {
            page.push(object);
        }
    }
}

/// Whether one upstream search result names a package this registry routes to
/// that upstream and this caller may read.
pub(super) fn search_result_is_visible(
    context: &UpstreamSearchContext<'_>,
    resolved: &RegistrySource,
    object: &Value,
) -> bool {
    let Some(name) = search_object_name(object) else {
        return false;
    };
    matches!(
        resolve_registry_source(context.state, context.registry, name),
        RegistrySource::Upstream(candidate) if candidate == context.source,
    ) && authorize(context.state, context.identity, resolved, name, Action::Access).is_ok()
}

pub(super) fn discovery_sources(
    state: &AppState,
    registry: &str,
    ecosystem: Ecosystem,
) -> Vec<DiscoverySource> {
    let registries = &state.inner.config.registries;
    registries
        .sources(registry, ecosystem)
        .into_iter()
        .filter_map(|source| match registries.get(source) {
            Some(Registry::Hosted { .. }) => Some(DiscoverySource::Hosted(source.to_string())),
            Some(Registry::Upstream { .. }) => Some(DiscoverySource::Upstream(source.to_string())),
            Some(Registry::Router { .. }) | None => None,
        })
        .collect()
}

pub(super) fn search_object_name(object: &Value) -> Option<&str> {
    object.get("package")?.get("name")?.as_str()
}

pub(super) fn upstream_search_query(query_string: &str, from: usize, size: usize) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in url::form_urlencoded::parse(query_string.as_bytes()) {
        if key != "from" && key != "size" {
            query.append_pair(&key, &value);
        }
    }
    query.append_pair("from", &from.to_string());
    query.append_pair("size", &size.clamp(1, 250).to_string());
    query.finish()
}
