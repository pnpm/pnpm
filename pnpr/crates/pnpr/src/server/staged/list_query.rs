pub(super) const DEFAULT_PER_PAGE: usize = 100;

pub(super) const MAX_PER_PAGE: usize = 100;

#[derive(Debug)]
pub(super) struct StagedListQuery {
    pub(super) page: usize,
    pub(super) per_page: usize,
    pub(super) package: Option<String>,
}

/// Parse the list endpoint's `page` / `perPage` / `package` query
/// parameters, ignoring anything unrecognized or unparsable.
pub(super) fn parse_staged_list_query(query: &str) -> StagedListQuery {
    let mut parsed = StagedListQuery { page: 0, per_page: DEFAULT_PER_PAGE, package: None };
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "page" => parsed.page = value.parse().unwrap_or(parsed.page),
            "perPage" => {
                parsed.per_page = value.parse().unwrap_or(parsed.per_page);
            }
            "package" if !value.is_empty() => parsed.package = Some(value.into_owned()),
            _ => {}
        }
    }
    parsed
}
