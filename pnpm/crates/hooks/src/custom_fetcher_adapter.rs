use std::sync::Arc;

use serde_json::Value;

use crate::{CustomFetcher, HookError};

/// Adapts a slice of [`CustomFetcher`] instances to a single "pick fetcher"
/// call: iterate the custom fetchers in declared order, return the first
/// one that claims the package via `can_fetch`.
///
/// This mirrors the TypeScript `pickFetcher` logic in
/// `pnpm11/fetching/pick-fetcher/src/index.ts`, where custom fetchers are
/// tried before built-in fetchers.
pub struct CustomFetcherPicker {
    fetchers: Vec<Arc<dyn CustomFetcher>>,
}

pub struct CustomFetcherSelection<'a> {
    pub fetcher: Option<&'a dyn CustomFetcher>,
    pub resolution: Value,
}

impl CustomFetcherPicker {
    #[must_use]
    pub fn new(fetchers: Vec<Arc<dyn CustomFetcher>>) -> Self {
        Self { fetchers }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fetchers.is_empty()
    }

    /// Consult each custom fetcher's `can_fetch` in declared order. Returns
    /// `Some(fetch_result)` from the first fetcher that claims the package,
    /// or `None` if no custom fetcher handles it (the caller falls through to
    /// built-in fetchers).
    pub async fn try_fetch(
        &self,
        pkg_id: &str,
        resolution: &Value,
        opts: &Value,
    ) -> Result<Option<Value>, HookError> {
        let CustomFetcherSelection { fetcher, resolution } =
            self.pick_fetcher(pkg_id, resolution).await?;
        let Some(fetcher) = fetcher else {
            return Ok(None);
        };
        fetcher.fetch(pkg_id, resolution, opts.clone()).await.map(Some)
    }

    pub async fn pick_fetcher(
        &self,
        pkg_id: &str,
        resolution: &Value,
    ) -> Result<CustomFetcherSelection<'_>, HookError> {
        let locked_integrity = resolution
            .get("type")
            .is_none_or(|kind| kind.is_null() || kind == "binary")
            .then(|| resolution.get("integrity"))
            .flatten()
            .filter(|value| value.as_str().is_some_and(|value| !value.is_empty()))
            .cloned();
        let mut resolution = resolution.clone();
        for fetcher in &self.fetchers {
            if !fetcher.has_can_fetch() || !fetcher.has_fetch() {
                continue;
            }
            let previous = resolution.clone();
            let (can_fetch, effective_resolution) =
                fetcher.can_fetch_with_resolution(pkg_id, resolution).await?;
            // `CustomFetcher` is a public trait, so an implementation can hand
            // back something that is not a resolution object. Keeping the
            // previous one leaves the locked-integrity restore below reachable
            // and stops a single bad answer from erasing the resolution for
            // every fetcher behind it.
            resolution =
                if effective_resolution.is_object() { effective_resolution } else { previous };
            if let Some(integrity) = &locked_integrity
                && let Some(object) = resolution.as_object_mut()
            {
                object.insert("integrity".to_owned(), integrity.clone());
            }
            if can_fetch {
                return Ok(CustomFetcherSelection { fetcher: Some(fetcher.as_ref()), resolution });
            }
        }
        Ok(CustomFetcherSelection { fetcher: None, resolution })
    }
}

#[cfg(test)]
mod tests;
