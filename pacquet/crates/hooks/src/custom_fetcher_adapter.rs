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
        for fetcher in &self.fetchers {
            if !fetcher.has_can_fetch() || !fetcher.has_fetch() {
                continue;
            }
            if fetcher.can_fetch(pkg_id, resolution.clone()).await? {
                let result = fetcher.fetch(pkg_id, resolution.clone(), opts.clone()).await?;
                return Ok(Some(result));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
