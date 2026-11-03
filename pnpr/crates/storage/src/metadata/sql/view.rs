//! The [`PackageMetadata`] of one `(catalog, ecosystem)` namespace.

use super::{Expected, PackageKey, SqlMetadata, commit, render, tags, transactions::Freshness};
use crate::{
    DocumentWrite, HostedDocumentForUpdate, HostedDocumentVersion,
    metadata::{PackageMetadata, PackageView, TagChange, TagUpdate},
};
use async_trait::async_trait;
use pnpr_error::Result;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::sync::Arc;

/// The metadata of one `(catalog, ecosystem)` namespace.
#[derive(Debug)]
pub(in crate::metadata) struct SqlView {
    pub(super) database: Arc<SqlMetadata>,
    pub(super) catalog: String,
    pub(super) ecosystem: Ecosystem,
}

impl SqlView {
    fn key(&self, name: &CanonicalPackageName) -> PackageKey {
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
}

#[async_trait]
impl PackageMetadata for SqlView {
    async fn read_view(&self, name: &CanonicalPackageName) -> Result<Option<PackageView>> {
        self.read_view_with(name, Freshness::Bounded).await
    }

    async fn read_view_at_least(
        &self,
        name: &CanonicalPackageName,
        generation: u64,
    ) -> Result<Option<PackageView>> {
        self.read_view_with(name, Freshness::AtLeast(generation)).await
    }

    async fn read_version(
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

    async fn read_release_record(
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

    async fn read_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        let Some(view) = self.read_view_with(name, Freshness::Current).await? else {
            return Ok(None);
        };
        let generation = view.generation.map_or(0, |generation| generation.value);
        Ok(Some(HostedDocumentForUpdate {
            bytes: view.bytes.to_vec(),
            version: HostedDocumentVersion::Generation(generation),
        }))
    }

    async fn write_if_current(
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

    async fn change_tag(
        &self,
        name: &CanonicalPackageName,
        change: TagChange<'_>,
    ) -> Result<TagUpdate> {
        let key = self.key(name);
        let change = tags::OwnedTagChange::from(change);
        self.database.write(move |connection| tags::change_tag(connection, &key, &change)).await
    }

    async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        let key = self.key(name);
        self.database.write(move |connection| commit::remove_package(connection, &key)).await
    }

    async fn package_names(&self) -> Result<Vec<String>> {
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
