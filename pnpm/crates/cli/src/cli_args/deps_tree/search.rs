//! Package search used by `pnpm list <pkg>` / `pnpm why <pkg>` and the
//! `--find-by` finder hooks. Rust counterpart of the TypeScript
//! tree-builder's `createPackagesSearcher`.

use std::collections::HashMap;

use node_semver::{Range, Version};
use pacquet_config::matcher::{Matcher, create_matcher};

use super::TreeNodeId;

/// Result of matching one package: no match, a plain match, or a match
/// with a message returned by a finder.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SearchMatch {
    No,
    Yes,
    Message(String),
}

impl SearchMatch {
    pub(crate) fn is_match(&self) -> bool {
        !matches!(self, SearchMatch::No)
    }

    pub(crate) fn message(&self) -> Option<&str> {
        match self {
            SearchMatch::Message(message) => Some(message),
            SearchMatch::No | SearchMatch::Yes => None,
        }
    }
}

struct ParsedQuery {
    match_name: Matcher,
    match_version: Option<Range>,
}

/// A compiled search: positional `<pkg>` queries evaluated inline, plus
/// finder verdicts pre-computed per `(alias, node)` pair (finders are
/// JavaScript callbacks running in the pnpmfile worker, so their
/// results are gathered before the synchronous tree walk).
pub(crate) struct Searcher {
    queries: Vec<ParsedQuery>,
    finder_results: HashMap<(String, Option<TreeNodeId>), SearchMatch>,
    has_finders: bool,
}

impl Searcher {
    pub(crate) fn from_queries(queries: &[String]) -> miette::Result<Self> {
        Ok(Searcher {
            queries: queries
                .iter()
                .map(|query| parse_search_query(query))
                .collect::<miette::Result<_>>()?,
            finder_results: HashMap::new(),
            has_finders: false,
        })
    }

    /// Record the verdicts of the `--find-by` finder callbacks,
    /// evaluated ahead of the tree walk. The key is the alias the
    /// package is referred to by and the node it resolves to (`None`
    /// for unresolvable leaf edges, keyed by alias only).
    pub(crate) fn set_finder_results(
        &mut self,
        results: HashMap<(String, Option<TreeNodeId>), SearchMatch>,
    ) {
        self.finder_results = results;
        self.has_finders = true;
    }

    pub(crate) fn matches(
        &self,
        alias: &str,
        name: &str,
        version: &str,
        node: Option<&TreeNodeId>,
    ) -> SearchMatch {
        for query in &self.queries {
            if !query.match_name.matches(name) && !query.match_name.matches(alias) {
                continue;
            }
            match &query.match_version {
                None => return SearchMatch::Yes,
                Some(range) => {
                    if !version.starts_with("link:")
                        && Version::parse(version).is_ok_and(|version| range.satisfies(&version))
                    {
                        return SearchMatch::Yes;
                    }
                }
            }
        }
        if self.has_finders {
            let key = (alias.to_string(), node.cloned());
            if let Some(verdict) = self.finder_results.get(&key) {
                return verdict.clone();
            }
        }
        SearchMatch::No
    }
}

fn parse_search_query(query: &str) -> miette::Result<ParsedQuery> {
    let (name, spec) = split_query(query);
    let match_name = create_matcher(std::slice::from_ref(&name.to_string()));
    let match_version = match spec {
        None => None,
        Some(spec) => Some(spec.parse::<Range>().map_err(|_| {
            miette::miette!("Invalid query - {query}. List can search only by version or range")
        })?),
    };
    Ok(ParsedQuery { match_name, match_version })
}

/// Split `<name>[@<spec>]`, honoring the `@scope/` prefix.
fn split_query(query: &str) -> (&str, Option<&str>) {
    let search_start = usize::from(query.starts_with('@'));
    match query[search_start..].find('@') {
        Some(at) => {
            let idx = search_start + at;
            (&query[..idx], Some(&query[idx + 1..]))
        }
        None => (query, None),
    }
}

#[cfg(test)]
mod tests;
