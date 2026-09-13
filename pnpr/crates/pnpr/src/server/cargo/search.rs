use super::{
    AppState, AuthedCaller, CanonicalPackageName, CrateDocument, DEFAULT_SEARCH_PAGE,
    DiscoverySource, ECOSYSTEM, Identity, RawQuery, RegistryError, Response, SearchCrate,
    SearchMeta, SearchPage, SearchResponse, State, StatusCode, TargetRegistry, addressed_registry,
    discovery_sources, error_response, hosted_search_names, json_response, not_found,
    private_no_cache,
};
pub(super) async fn get_search(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    RawQuery(query): RawQuery,
) -> Response {
    let query_string = query.unwrap_or_default();
    let respond = |crates: Vec<SearchCrate>, total: usize| {
        json_response(
            StatusCode::OK,
            &serde_json::to_value(SearchResponse {
                crates,
                meta: SearchMeta {
                    total,
                },
            })
            .expect("search response serializes"),
        )
    };
    let Some(text) = pnpr_search::parse_query(&query_string).map(pnpr_search::SearchText::Package)
    else {
        return respond(Vec::new(), 0);
    };
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
    };
    let size = pnpr_search::parse_usize_param(&query_string, "per_page")
        .map_or(DEFAULT_SEARCH_PAGE, |size| {
            size.clamp(1, pnpr_search::MAX_PAGE_SIZE)
        });
    // crates.io numbers pages from one; anything lower starts at the first.
    let from = pnpr_search::parse_usize_param(&query_string, "page")
        .map_or(0, |page| page.saturating_sub(1).saturating_mul(size));

    let mut page = SearchPage::new(from, size);
    if let Err(err) = collect_hosted_crates(&state, &identity, &target, &text, &mut page)
        .await
    {
        return error_response(err);
    }
    let total = page.total();
    // Results are filtered per caller (registry access plus per-package
    // ACL), so they must never land in a shared HTTP cache.
    private_no_cache(respond(page.objects, total))
}

/// Add every hosted registry's matching crates to the page.
async fn collect_hosted_crates(
    state: &AppState,
    identity: &Identity,
    target: &str,
    text: &pnpr_search::SearchText,
    page: &mut SearchPage<SearchCrate>,
) -> Result<(), RegistryError> {
    for source in discovery_sources(state, target, ECOSYSTEM) {
        let DiscoverySource::Hosted(source) = source else {
            continue;
        };
        let hosted = hosted_search_names(state, identity, target, &source, ECOSYSTEM, text)
            .await;
        let (storage, names) = match hosted {
            Ok(Some(hosted)) => hosted,
            Ok(None) => continue,
            Err(err) => return Err(err),
        };
        add_crates_to_page(page, &storage, names).await;
    }
    Ok(())
}

/// `GET api/v1/crates?q=<query>&per_page=<n>&page=<n>` — `cargo search`.
///
/// Hosted sources only. An upstream contributes nothing, the way an npm
/// upstream does until its `search` is turned on, and searching one needs
/// its `config.json` `api` base rather than the index base pnpr proxies.
/// Add one hosted source's names to the page.
///
/// A hosted namespace shared with another ecosystem holds names that are not
/// crate names. Dropping them before the position is claimed keeps them out of
/// the page and out of the total, and costs no read.
async fn add_crates_to_page(
    page: &mut SearchPage<SearchCrate>,
    storage: &pnpr_storage::Storage,
    names: Vec<String>,
) {
    for name in names {
        let Ok(key) = CanonicalPackageName::parse(&name, ECOSYSTEM) else {
            continue;
        };
        if page.push_name(&name) {
            page.objects.push(search_crate(storage, &key).await);
        }
    }
}

/// One search row, read from the crate's stored document. A document that
/// cannot be read or parsed still answers with the name the caller matched,
/// so the page never disagrees with the total it reports.
async fn search_crate(storage: &pnpr_storage::Storage, key: &CanonicalPackageName) -> SearchCrate {
    let document = async {
        let bytes = storage.read_hosted_document(key).await.ok()??;
        CrateDocument::parse(&bytes).ok()
    }
    .await;
    document
        .as_ref()
        .map_or_else(
            || SearchCrate {
                name: key.as_str().to_string(),
                description: None,
                max_version: String::new(),
            },
            CrateDocument::to_search_crate,
        )
}
