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
    /// A source's reported total minus what was walked, not deduplicated
    /// against `names`. Counted at the tail of the served sequence, behind
    /// every downloaded result of every source.
    pub(super) unscanned: usize,
}

impl<Item> SearchPage<Item> {
    pub(super) fn new(from: usize, size: usize) -> Self {
        Self { objects: Vec::new(), names: HashSet::new(), from, size, unscanned: 0 }
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
        self.names.len().saturating_add(self.unscanned)
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

/// The ceiling on upstream requests for one search, however many sources a
/// registry routes to. Continuation pages stop at
/// [`MAX_UPSTREAM_SEARCH_PAGES`]; the fetch each source is guaranteed may
/// carry the search past that, but never past this.
pub(super) const MAX_UPSTREAM_SEARCH_REQUESTS: usize = 32;

/// What one search may download from its upstreams. The budgets bound
/// pnpr's own fetching, never what an upstream advertises: npmjs's loose
/// full-text search reports five-digit totals for almost any term, so a
/// search that refused those would refuse almost every term.
#[derive(Default)]
pub(super) struct UpstreamSearchBudget {
    pub(super) pages: usize,
    pub(super) requests: usize,
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

    pub(super) fn try_take_page(&mut self) -> bool {
        if self.pages == MAX_UPSTREAM_SEARCH_PAGES {
            return false;
        }
        self.pages += 1;
        true
    }

    pub(super) fn try_take_request(&mut self) -> bool {
        if self.requests == MAX_UPSTREAM_SEARCH_REQUESTS {
            return false;
        }
        self.requests += 1;
        true
    }

    pub(super) fn add_results(&mut self, object_count: usize) {
        self.results = self.results.saturating_add(object_count);
    }
}

/// `GET /-/v1/search?text=...&from=...&size=...` — npm search v1 endpoint.
/// Hosted results are counted after routing and access filters, then optional
/// upstream results are appended in registry-source order. An upstream only
/// participates when its `search` setting is enabled, and only within the
/// [`UpstreamSearchBudget`].
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
                let search =
                    UpstreamSearch { registry: &registry, source: &source, query_string, browse };
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
    let Some(config) = state.inner.config.routing.upstreams.get(search.source) else {
        return Ok(());
    };
    if search.browse || !config.search || !upstream_search_admits(config, identity) {
        return Ok(());
    }
    let Some(upstream) = state.inner.proxy.upstreams.get(search.source) else {
        return Ok(());
    };
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
    config.access
        .as_ref()
        .is_none_or(|access| access.allows(identity))
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
    let Some(hosted) = state.inner.config.routing.hosted.get(source) else {
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
    // Unconditional against the page budget, though not the request cap: a
    // source routed after a large one must still be asked, or it vanishes
    // from `objects` and `total` at once.
    budget.try_take_page();
    let mut from = 0usize;
    loop {
        if !budget.try_take_request() {
            return Ok(());
        }
        let size = budget.remaining_results().clamp(1, FETCH_SIZE);
        let query = upstream_search_query(context.query_string, from, size);
        let response = match context.upstream.fetch_search(&query).await? {
            FetchOutcome::Ok(response) => response,
            FetchOutcome::NotFound => return Ok(()),
        };
        match consume_upstream_page(&context, &resolved, response, page, budget, &mut from)? {
            PageOutcome::Done => return Ok(()),
            PageOutcome::More => {}
        }
    }
}

pub(super) enum PageOutcome {
    Done,
    More,
}

/// Advances `from` by what the result budget let this page keep.
pub(super) fn consume_upstream_page(
    context: &UpstreamSearchContext<'_>,
    resolved: &RegistrySource,
    response: pnpr_upstream::SearchResponse,
    page: &mut SearchPage<Value>,
    budget: &mut UpstreamSearchBudget,
    from: &mut usize,
) -> Result<PageOutcome, RegistryError> {
    let fetched = response.objects.len();
    let mut objects = response.objects;
    objects.truncate(budget.remaining_results());
    let consumed = objects.len();
    budget.add_results(consumed);
    append_visible_results(context, resolved, objects, page);
    *from = from.saturating_add(consumed);
    if *from >= response.total {
        return Ok(PageOutcome::Done);
    }
    if fetched == 0 {
        return Err(RegistryError::UpstreamResponse {
            url: format!("{}/-/v1/search", context.source),
            reason: format!("reported {} results but returned an empty page", response.total),
        });
    }
    if budget.remaining_results() == 0 || !budget.try_take_page() {
        // Raw, unfiltered by `search_result_is_visible`, yet no leak:
        // `upstream_search_admits` withholds a source from any caller its
        // access or package rules deny.
        page.unscanned = page.unscanned.saturating_add(response.total.saturating_sub(*from));
        return Ok(PageOutcome::Done);
    }
    Ok(PageOutcome::More)
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
    let registries = &state.inner.config.routing.registries;
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
    object
        .get("package")?
        .get("name")?
        .as_str()
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
