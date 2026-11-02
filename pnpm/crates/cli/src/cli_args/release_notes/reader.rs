//! The notes between an installed version and the one an upgrade offers.

use super::http::{NotesHttp, VersionNoteResponse, publication_url};
use node_semver::Version;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_release_notes::{NoteEntry, NotesList, is_in_upgrade_range};
use std::{collections::HashMap, sync::Mutex};

/// The notes of an upgrade: version notes newest first, and the
/// publication notes they came out of, each once.
#[derive(Debug, Default)]
pub(crate) struct UpgradeNotes {
    pub versions: Vec<VersionNote>,
    pub publications: Vec<PublicationNote>,
}

#[derive(Debug)]
pub(crate) struct VersionNote {
    pub version: String,
    pub text: Option<String>,
    /// Index into [`UpgradeNotes::publications`].
    pub publication: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct PublicationNote {
    pub url: String,
    pub name: Option<String>,
    pub text: String,
}

impl UpgradeNotes {
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}

/// Reads notes for one command run. A publication note is fetched once,
/// however many upgraded packages came out of it.
pub(crate) struct NotesReader<'a> {
    http: NotesHttp<'a>,
    publications: Mutex<HashMap<String, Option<String>>>,
}

/// A version in an upgrade's range, before its publication note is read.
struct RangeEntry {
    version: String,
    text: Option<String>,
    publication: Option<(String, Option<String>)>,
}

impl<'a> NotesReader<'a> {
    pub fn new(client: &'a ThrottledClient, config: &'a Config) -> Self {
        Self { http: NotesHttp { client, config }, publications: Mutex::default() }
    }

    /// The notes of `name` for `installed < version <= target`, read from
    /// `<name>/-/notes`, and one version at a time past a truncated list.
    pub async fn upgrade_notes(
        &self,
        registry: &str,
        name: &str,
        installed: &Version,
        target: &Version,
    ) -> miette::Result<UpgradeNotes> {
        if !pnpm_package_name::is_valid_old_npm_package_name(name) {
            return Ok(UpgradeNotes::default());
        }
        let list = self.http.notes_list(registry, name).await?.unwrap_or_default();
        let mut entries: Vec<RangeEntry> = list.notes
            .iter()
            .filter(|entry| in_range(&entry.version, installed, target))
            .map(|entry| range_entry(registry, entry))
            .collect();
        if list.truncated {
            entries.extend(self.older_than_list(registry, name, &list, installed, target).await?);
        }
        self.with_publications(entries, name).await
    }

    /// The note of `name@version` and its publication's.
    pub async fn version_notes(
        &self,
        registry: &str,
        name: &str,
        version: &str,
    ) -> miette::Result<UpgradeNotes> {
        if !pnpm_package_name::is_valid_old_npm_package_name(name) {
            return Ok(UpgradeNotes::default());
        }
        let response = self.http.version_note(registry, name, version).await?;
        let entries = response
            .map(|response| version_range_entry(version.to_owned(), response))
            .into_iter()
            .collect();
        self.with_publications(entries, name).await
    }

    /// The versions in range that a truncated list leaves out, each read
    /// from its own route.
    async fn older_than_list(
        &self,
        registry: &str,
        name: &str,
        list: &NotesList,
        installed: &Version,
        target: &Version,
    ) -> miette::Result<Vec<RangeEntry>> {
        let oldest_listed = list.notes
            .iter()
            .filter_map(|entry| entry.version.parse::<Version>().ok())
            .min();
        let package = pnpm_registry::Package::fetch_from_registry(
            name,
            self.http.client,
            registry,
            &self.http.config.auth_headers,
        )
        .await
        .map_err(miette::Report::new)?;
        let mut versions: Vec<Version> = package.versions
            .keys()
            .filter_map(|key| key.parse::<Version>().ok())
            .filter(|version| is_in_upgrade_range(version, installed, target))
            .filter(|version| {
                oldest_listed
                    .as_ref()
                    .is_none_or(|oldest| version < oldest)
            })
            .collect();
        versions.sort_by(|left, right| right.cmp(left));
        let reads = versions
            .iter()
            .map(|version| {
                let version = version.to_string();
                async move {
                    let response = self.http.version_note(registry, name, &version).await?;
                    Ok::<_, miette::Report>(response.map(|response| {
                        version_range_entry(version, response)
                    }))
                }
            });
        let responses = futures_util::future::try_join_all(reads).await?;
        Ok(responses
            .into_iter()
            .flatten()
            .collect())
    }

    async fn with_publications(
        &self,
        entries: Vec<RangeEntry>,
        package: &str,
    ) -> miette::Result<UpgradeNotes> {
        let mut notes = UpgradeNotes::default();
        for entry in entries {
            let publication = match entry.publication {
                Some((url, name)) => self.publication_index(&mut notes, url, name, package).await?,
                None => None,
            };
            if entry.text.is_some() || publication.is_some() {
                notes.versions.push(VersionNote {
                    version: entry.version,
                    text: entry.text,
                    publication,
                });
            }
        }
        Ok(notes)
    }

    async fn publication_index(
        &self,
        notes: &mut UpgradeNotes,
        url: String,
        name: Option<String>,
        package: &str,
    ) -> miette::Result<Option<usize>> {
        if let Some(index) = notes.publications
            .iter()
            .position(|known| known.url == url)
        {
            return Ok(Some(index));
        }
        let Some(text) = self.publication_text(&url, package).await? else {
            return Ok(None);
        };
        notes.publications.push(PublicationNote { url, name, text });
        Ok(Some(notes.publications.len() - 1))
    }

    async fn publication_text(&self, url: &str, package: &str) -> miette::Result<Option<String>> {
        let cached = self.publications
            .lock()
            .expect("publication cache lock")
            .get(url)
            .cloned();
        if let Some(text) = cached {
            return Ok(text);
        }
        let text = self.http.publication_note(url, package).await?.map(|(text, _)| text);
        self.publications
            .lock()
            .expect("publication cache lock")
            .insert(url.to_owned(), text.clone());
        Ok(text)
    }
}

fn in_range(version: &str, installed: &Version, target: &Version) -> bool {
    version.parse::<Version>().is_ok_and(|version| is_in_upgrade_range(&version, installed, target))
}

fn range_entry(registry: &str, entry: &NoteEntry) -> RangeEntry {
    let publication = entry.publication
        .as_ref()
        .and_then(|publication| {
            publication_url(registry, publication).map(|url| (url, publication.name.clone()))
        });
    RangeEntry { version: entry.version.clone(), text: entry.text.clone(), publication }
}

fn version_range_entry(version: String, response: VersionNoteResponse) -> RangeEntry {
    RangeEntry {
        version,
        text: response.text,
        publication: response.publication.map(|link| (link.url, link.name)),
    }
}
