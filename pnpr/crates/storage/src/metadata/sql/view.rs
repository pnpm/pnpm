//! The [`PackageMetadata`] of one `(catalog, ecosystem)` namespace.

use super::{
    Expected, PackageKey, SqlMetadata, blobs::LookupRead, commit, render, tags,
    transactions::Freshness,
};
use crate::{
    DocumentWrite, HostedDocumentForUpdate, HostedDocumentVersion,
    metadata::{BlobLookup, PackageView, PublishBase, TagChange, TagUpdate},
};
use pnpr_error::Result;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::sync::Arc;

/// The metadata of one `(catalog, ecosystem)` namespace: what the registry's
/// handlers reach through [`crate::Storage`] to read a package view, commit a
/// package document, change a tag, or remove a package.
#[derive(Debug, Clone)]
pub(crate) struct PackageMetadata {
    pub(super) database: Arc<SqlMetadata>,
    pub(super) catalog: String,
    pub(super) ecosystem: Ecosystem,
}

impl PackageMetadata {
    /// The catalog of this namespace.
    pub(crate) fn catalog(&self) -> &str {
        &self.catalog
    }

    pub(crate) fn ecosystem(&self) -> Ecosystem {
        self.ecosystem
    }

    pub(super) fn key(&self, name: &CanonicalPackageName) -> PackageKey {
        PackageKey {
            catalog: self.catalog.clone(),
            ecosystem: self.ecosystem,
            name: name.as_str().to_string(),
        }
    }

    async fn read_view_with(
        &self,
        name: &CanonicalPackageName,
        freshness: Freshness,
    ) -> Result<Option<PackageView>> {
        let key = self.key(name);
        self.database.read(self.catalog.clone(), freshness, move |connection, cache| {
            render::read_view(connection, cache, &key)
        })
        .await
    }

    /// The package's document as a reader receives it, with the generation
    /// it was rendered from.
    pub(crate) async fn read_view(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<PackageView>> {
        self.read_view_with(name, Freshness::Bounded).await
    }

    /// [`Self::read_view`] at catalog generation `generation` or later, such
    /// as one a client has already seen.
    pub(crate) async fn read_view_at_least(
        &self,
        name: &CanonicalPackageName,
        generation: u64,
    ) -> Result<Option<PackageView>> {
        self.read_view_with(name, Freshness::AtLeast(generation)).await
    }

    /// One version's accepted manifest, with its `_archiveManifest`
    /// interpretation but without its README and deprecation: what the
    /// version's release record is made of. Only that version's rows are
    /// read. `None` when the package has no such npm version.
    pub(crate) async fn read_version(
        &self,
        name: &CanonicalPackageName,
        version: &str,
    ) -> Result<Option<Arc<str>>> {
        if self.ecosystem != Ecosystem::Npm {
            return Ok(None);
        }
        let key = self.key(name);
        let version = version.to_string();
        self.database.read(self.catalog.clone(), Freshness::Bounded, move |connection, cache| {
            render::read_version(connection, cache, &key, &version)
        })
        .await
    }

    /// The exact bytes of the release record of `revision` of `version`, as
    /// accepted with the version, or `None` when the store holds none for it.
    pub(crate) async fn read_release_record(
        &self,
        name: &CanonicalPackageName,
        version: &str,
        revision: u64,
    ) -> Result<Option<Vec<u8>>> {
        // Every version is outside the revision protocol here, so its one
        // record is revision 0's.
        if self.ecosystem != Ecosystem::Npm || revision != 0 {
            return Ok(None);
        }
        let key = self.key(name);
        let version = version.to_string();
        self.database.read(self.catalog.clone(), Freshness::Bounded, move |connection, _| {
            super::versions::read_release_record(connection, &key, &version)
        })
        .await
    }

    /// Read a document with the generation [`Self::write_if_current`] needs
    /// to detect a concurrent writer.
    pub(crate) async fn read_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        let Some(view) = self.read_view_with(name, Freshness::Current).await? else {
            return Ok(None);
        };
        let generation = view.generation.value;
        Ok(Some(HostedDocumentForUpdate {
            bytes: view.bytes.to_vec(),
            version: HostedDocumentVersion::Generation(generation),
        }))
    }

    /// [`Self::read_for_update`] for a publish, with the rows `lookup` asks
    /// for read in the same round trip: those of the blobs the publish is
    /// about to store, and the mappings of files it references.
    pub(crate) async fn read_for_publish(
        &self,
        name: &CanonicalPackageName,
        lookup: &BlobLookup,
    ) -> Result<PublishBase> {
        let key = self.key(name);
        let read = LookupRead::new(key.clone(), lookup);
        let (view, (blobs, artifacts)) = self.database.read(
            self.catalog.clone(),
            Freshness::Current,
            move |connection, cache| {
                let (view, rows) = render::read_view_with(connection, cache, &key, read.queries())?;
                Ok((view, read.answer(&mut rows.into_iter())?))
            },
        )
        .await?;
        let stored = view.map(|view| HostedDocumentForUpdate {
            bytes: view.bytes.to_vec(),
            version: HostedDocumentVersion::Generation(view.generation.value),
        });
        Ok(PublishBase { stored, blobs, artifacts })
    }

    /// Commit `bytes` as the package's document while the stored one is still
    /// at `version`: absent for `None`, any for
    /// [`HostedDocumentVersion::Unversioned`]. `principal` is who asked for
    /// the write, recorded with every tag it moves.
    pub(crate) async fn write_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
        principal: Option<&str>,
    ) -> Result<DocumentWrite> {
        let expected = Expected::from_version(version)?;
        let key = self.key(name);
        let bytes = bytes.to_vec();
        let principal = principal.map(str::to_string);
        self.database.write(move |connection| {
            let principal = principal.as_deref();
            commit::write_document(connection, &key, &bytes, expected, principal)
        })
        .await
    }

    /// Move or remove a dist-tag in one commit.
    pub(crate) async fn change_tag(
        &self,
        name: &CanonicalPackageName,
        change: TagChange<'_>,
    ) -> Result<TagUpdate> {
        let key = self.key(name);
        let change = tags::OwnedTagChange::from(change);
        self.database.write(move |connection| tags::change_tag(connection, &key, &change)).await
    }

    /// Remove the package's metadata. Returns the blobs its files were
    /// mapped to, `None` when there was no metadata.
    pub(crate) async fn remove_package(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<crate::BlobId>>> {
        let key = self.key(name);
        self.database.write(move |connection| commit::remove_package(connection, &key)).await
    }

    /// The names of every package with metadata, sorted.
    pub(crate) async fn package_names(&self) -> Result<Vec<String>> {
        let (catalog, ecosystem) = (self.catalog.clone(), self.ecosystem);
        self.database.read(catalog.clone(), Freshness::Bounded, move |connection, _| {
            connection
                .query(
                    "SELECT name FROM packages WHERE catalog = ?1 AND ecosystem = ?2 ORDER BY name",
                    params![catalog, ecosystem.as_str()],
                )?
                .into_iter()
                .map(|mut row| row.get(0))
                .collect()
        })
        .await
    }
}
