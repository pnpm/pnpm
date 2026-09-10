use super::{
    Action, CanonicalPackageName, Catalog, ECOSYSTEM, Ecosystem, ErrorCode, ImageDocument, Method,
    Refusal, RegistryError, RegistrySource, Request, Response, StatusCode, TagList,
    addressed_registry, authorize, error, hosted_sources, insert_header, json, method_not_allowed,
    private_no_cache, query_param, read_hosted_document, registry_error, resolve_ecosystem_source,
    unknown_repository,
};

impl Request {
    pub(super) fn page_size(&self) -> Result<Option<usize>, Refusal> {
        query_param(Some(&self.query), "n")
            .map(|value| {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(Refusal::new(
                        ErrorCode::NameInvalid,
                        "n must be a non-negative integer",
                    ));
                }
                value
                    .parse::<usize>()
                    .map_err(|_| Refusal::new(ErrorCode::NameInvalid, "n is too large"))
            })
            .transpose()
    }

    pub(super) fn paginate<Item: AsRef<str>>(
        &self,
        items: &mut Vec<Item>,
        endpoint: &str,
    ) -> Result<Option<String>, Refusal> {
        let count = self.page_size()?;
        if let Some(last) = query_param(Some(&self.query), "last") {
            items.retain(|item| item.as_ref() > last.as_str());
        }
        let Some(count) = count else { return Ok(None) };
        let more = items.len() > count;
        items.truncate(count);
        Ok(if more && let Some(last) = items.last() {
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("n", &count.to_string())
                .append_pair("last", last.as_ref())
                .finish();
            Some(format!(r#"<{}/{endpoint}?{query}>; rel="next""#, self.base))
        } else {
            None
        })
    }

    /// `GET /v2/_catalog` — the repository names this caller may read.
    pub(super) async fn catalog(&self) -> Response {
        if self.method != Method::GET {
            return method_not_allowed();
        }
        let Some(target) =
            addressed_registry(&self.state, self.registry.as_deref(), Ecosystem::Oci)
        else {
            return error(ErrorCode::NameUnknown, "no registry is addressed here");
        };
        match self.page_size() {
            Ok(Some(0)) => {
                return private_no_cache(json(
                    StatusCode::OK,
                    &Catalog { repositories: Vec::new() },
                ));
            }
            Ok(_) => {}
            Err(refusal) => return refusal.respond(),
        }
        let last = query_param(Some(&self.query), "last");
        let mut repositories = Vec::new();
        for source in hosted_sources(&self.state, &target, ECOSYSTEM) {
            match self.readable_repositories(&target, &source, last.as_deref()).await {
                Ok(names) => repositories.extend(names),
                Err(err) => return registry_error(err),
            }
        }
        repositories.sort();
        repositories.dedup();
        // The listing is built from what this caller may read, so it is
        // caller-specific whichever registry it came through.
        let link = match self.paginate(&mut repositories, "_catalog") {
            Ok(link) => link,
            Err(refusal) => return refusal.respond(),
        };
        let mut response = json(StatusCode::OK, &Catalog { repositories });
        if let Some(link) = link {
            insert_header(&mut response, "link", &link);
        }
        private_no_cache(response)
    }

    /// `GET /v2/<name>/tags/list`.
    pub(super) async fn tags(&self, name: &str) -> Response {
        if self.method != Method::GET {
            return method_not_allowed();
        }
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        let response =
            match read_hosted_document::<ImageDocument>(&self.state, &self.identity, &source, &key)
                .await
            {
                Ok(Some(document)) => {
                    let mut tags = document.tag_names();
                    let link =
                        match self.paginate(&mut tags, &format!("{}/tags/list", key.as_str())) {
                            Ok(link) => link,
                            Err(refusal) => return refusal.respond(),
                        };
                    let mut response = json(StatusCode::OK, &TagList { name: key.as_str(), tags });
                    if let Some(link) = link {
                        insert_header(&mut response, "link", &link);
                    }
                    response
                }
                Ok(None) => unknown_repository(name).respond(),
                Err(err) => registry_error(err),
            };
        self.caller_scoped(Some(key.as_str()), response)
    }

    /// The repositories of one hosted registry this caller may list, after
    /// `last`.
    pub(super) async fn readable_repositories(
        &self,
        target: &str,
        source: &str,
        last: Option<&str>,
    ) -> Result<Vec<String>, RegistryError> {
        let Some(hosted) = self.state.inner.config.hosted.get(source) else {
            return Ok(Vec::new());
        };
        let storage = self.state.inner.storage.for_hosted(&hosted.org);
        let names = storage.hosted_package_names().await?;
        // A listing may only name what this caller could have fetched.
        Ok(names
            .into_iter()
            .filter(|name| {
            last.is_none_or(|last| name.as_str() > last)
                && CanonicalPackageName::parse(name, ECOSYSTEM).is_ok()
                && matches!(resolve_ecosystem_source(&self.state, target, ECOSYSTEM, name), RegistrySource::Hosted(ref resolved) if resolved == source)
                && authorize(
                    &self.state,
                    &self.identity,
                    &RegistrySource::Hosted(source.to_string()),
                    name,
                    Action::Access,
                )
                .is_ok()
        })
            .collect())
    }
}
