use std::{borrow::Cow, collections::HashSet};

/// The command names [`super::choose_bins`] must not link. Windows
/// resolves commands without regard to case, so there the names are
/// lowercased once and each lookup is a single hash probe.
pub(super) struct ExcludedBins<'names>(Cow<'names, HashSet<String>>);

impl<'names> ExcludedBins<'names> {
    pub(super) fn new(names: &'names HashSet<String>) -> Self {
        if cfg!(windows) {
            Self(Cow::Owned(
                names
                    .iter()
                    .map(|name| name.to_lowercase())
                    .collect(),
            ))
        } else {
            Self(Cow::Borrowed(names))
        }
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        if cfg!(windows) { self.0.contains(&name.to_lowercase()) } else { self.0.contains(name) }
    }
}
