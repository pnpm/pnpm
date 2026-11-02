//! The release notes a publish carries to a registry that stores them: a
//! top-level `notes` field in each package's document, and the
//! publication's `name` and `notes` at the top of a batch body.

use crate::registry_config_keys::NormalizedRegistryUrl;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

/// Where release notes are sent, and the publication's own name and note.
#[derive(Debug, Default)]
pub struct ReleaseNotesTarget {
    /// The normalized URLs of the registries declared `serverType: pnpr`.
    /// Every other registry gets the same documents without notes.
    pub registries: HashSet<String>,
    pub publication: PublicationNotes,
}

/// The top-level fields of a batch publish to a registry that stores notes.
#[derive(Debug, Default, Clone)]
pub struct PublicationNotes {
    pub name: Option<String>,
    pub notes: Option<String>,
}

impl ReleaseNotesTarget {
    #[must_use]
    pub fn stores_notes(&self, registry: &NormalizedRegistryUrl) -> bool {
        self.registries.contains(registry.as_str())
    }

    /// Put the version note into a package's publish document bound for
    /// `registry`.
    pub(crate) fn attach_version_notes(
        &self,
        document: &mut Value,
        notes: Option<&str>,
        registry: &NormalizedRegistryUrl,
    ) {
        let (Some(notes), Some(document)) = (notes, document.as_object_mut()) else {
            return;
        };
        if self.stores_notes(registry) {
            document.insert("notes".to_owned(), Value::String(notes.to_owned()));
        }
    }

    /// The batch body for `registry`: the package documents, with the
    /// publication's name and note when the registry stores notes.
    pub(crate) fn batch_body<'a>(
        &'a self,
        registry: &NormalizedRegistryUrl,
        documents: &'a [Value],
    ) -> BatchBody<'a> {
        let publication = self.stores_notes(registry).then_some(&self.publication);
        BatchBody {
            name: publication.and_then(|publication| publication.name.as_deref()),
            notes: publication.and_then(|publication| publication.notes.as_deref()),
            packages: documents,
        }
    }
}

/// The body of a batch publish request.
#[derive(Serialize)]
pub(crate) struct BatchBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<&'a str>,
    packages: &'a [Value],
}

#[cfg(test)]
mod tests;
