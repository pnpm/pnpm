use super::{BoundProviders, PkgName, SnapshotDepRef, binds_reference, names_provider};

#[derive(Clone, PartialEq, Eq)]
pub(in crate::next_format::write) struct ProviderConstraint {
    alias: PkgName,
    bindings: Bindings,
}

#[derive(Clone, PartialEq, Eq)]
enum Bindings {
    Named(Vec<String>),
    Recovered(Option<BoundProviders>),
}

impl ProviderConstraint {
    pub(in crate::next_format::write) fn new(
        alias: &PkgName,
        segments: Option<&[String]>,
        recovered: Option<&BoundProviders>,
    ) -> Self {
        let bindings = match segments {
            Some(segments) => Bindings::Named(segments.to_vec()),
            None => Bindings::Recovered(recovered.cloned()),
        };
        ProviderConstraint { alias: alias.clone(), bindings }
    }

    pub(in crate::next_format::write) fn matches(&self, reference: &SnapshotDepRef) -> bool {
        match &self.bindings {
            Bindings::Named(segments) => names_provider(segments, &self.alias, reference),
            Bindings::Recovered(providers) => {
                binds_reference(providers.as_ref(), &self.alias, reference)
            }
        }
    }
}

#[cfg(test)]
mod tests;
