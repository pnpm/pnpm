//! The layout marker of a hosted store, and the proxy cache's
//! wipe-and-rebuild rule.

use crate::{
    HostedBackend, Result, Storage,
    layout::{LAYOUT_FILE, LAYOUT_VERSION, LayoutMarker, new_store_id},
    metadata::sql::StoreBinding,
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;
use std::sync::Arc;
use tokio::fs;

/// The record namespace the marker is written in: the store root itself.
const ROOT: &str = "";

/// Which layout a hosted store is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedLayout {
    /// [`LAYOUT_VERSION`], with the store's `store_id` when `layout.json`
    /// names one.
    Current { store_id: Option<String> },
    /// Marked with another layout version.
    Other(u32),
    /// No marker. `has_data` says whether anything but shared records is
    /// stored, which would be in the layout from before markers existed.
    Unmarked { has_data: bool },
}

enum MarkerClaim {
    Created(String),
    Existing(String),
}

impl MarkerClaim {
    fn store_id(&self) -> &str {
        match self {
            Self::Created(id) | Self::Existing(id) => id,
        }
    }
}

impl Storage {
    /// A view of `(org, ecosystem)`'s hosted namespace, so two registries
    /// hosting the same `name@version` can't collide. The empty org is the
    /// default namespace, and `("", Ecosystem::Npm)` is the one
    /// [`Storage::new`] addresses. The disposable proxy cache is shared.
    #[must_use]
    pub fn for_hosted(&self, org: &str, ecosystem: Ecosystem) -> Storage {
        let hosted = namespace(&self.root, org, ecosystem);
        Storage {
            root: Arc::clone(&self.root),
            hosted,
            cached: self.cached.clone(),
            tar_integrities: Arc::clone(&self.tar_integrities),
            tar_integrity_cache: Arc::clone(&self.tar_integrity_cache),
            database: Arc::clone(&self.database),
            metadata: self.database.view(org, ecosystem),
        }
    }

    /// Which layout the hosted store is in.
    ///
    /// # Errors
    ///
    /// The store is unreachable, or its marker is unreadable.
    pub async fn hosted_layout(&self) -> Result<HostedLayout> {
        if let Some(bytes) = self.root.read_record(ROOT, LAYOUT_FILE).await? {
            let marker = parse_marker(&bytes)?;
            return Ok(if marker.layout == LAYOUT_VERSION {
                HostedLayout::Current { store_id: marker.store_id }
            } else {
                HostedLayout::Other(marker.layout)
            });
        }
        let has_data = self.root
            .list_top_level_dirs()
            .await?
            .iter()
            .any(|dir| !dir.starts_with('.'));
        Ok(HostedLayout::Unmarked { has_data })
    }

    /// Check the hosted store's layout and database binding before anything
    /// writes it, writing the marker and binding the metadata database when
    /// both hold no packages yet.
    ///
    /// # Errors
    ///
    /// See [`Self::check_hosted_layout`].
    pub async fn ensure_hosted_layout(&self) -> Result<()> {
        if matches!(self.check_hosted_layout().await?, HostedLayout::Current { store_id: Some(_) })
        {
            return Ok(());
        }
        self.write_hosted_layout_marker().await
    }

    /// Check that this pnpr can read the hosted store and that its `store_id`
    /// matches the metadata database, without writing to either. Returns the
    /// layout, which is [`HostedLayout::Current`] with a matching `store_id`
    /// or an empty unmarked store with an unbound database.
    ///
    /// # Errors
    ///
    /// A store written by a newer pnpr, one in an earlier layout, or a store
    /// and metadata database whose `store_id`s differ or are present on only
    /// one side.
    pub async fn check_hosted_layout(&self) -> Result<HostedLayout> {
        let layout = self.hosted_layout().await?;
        let binding = self.database.store_binding().await?;
        verify_store_binding(layout, binding)
    }

    /// Prepare the target store of `migrate-storage` to receive the archives of
    /// `source_layout`, which was checked against the shared metadata database.
    ///
    /// # Errors
    ///
    /// The target store is bound to another store id, holds data without a
    /// marker, or uses another layout version.
    pub async fn ensure_migration_target_layout(&self, source_layout: &HostedLayout) -> Result<()> {
        let Some(store_id) = (match source_layout {
            HostedLayout::Current { store_id: Some(store_id) } => Some(store_id.as_str()),
            _ => None,
        }) else {
            return self.ensure_hosted_layout().await;
        };
        match self.hosted_layout().await? {
            HostedLayout::Current { store_id: Some(target_id) } if target_id == store_id => Ok(()),
            HostedLayout::Current { store_id: Some(target_id) } => {
                Err(store_mismatch_error(&target_id, store_id))
            }
            HostedLayout::Current { store_id: None } => Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the metadata database is bound to hosted store {store_id}, but the hosted store's {LAYOUT_FILE} has no store_id",
                ),
            }),
            HostedLayout::Unmarked { has_data: true } => Err(unmarked_with_data_error()),
            HostedLayout::Other(layout) => Err(other_layout_error(layout)),
            HostedLayout::Unmarked { has_data: false } => {
                self.write_hosted_layout_marker_for(store_id).await
            }
        }
    }

    /// Mark the hosted store as being in [`LAYOUT_VERSION`] and bind the
    /// metadata database to its `store_id`. Only for an empty store and
    /// database.
    pub async fn write_hosted_layout_marker(&self) -> Result<()> {
        let claim = self.claim_marker_store_id().await?;
        let store_id = claim.store_id();
        match self.database.claim_store_id(store_id.to_string()).await {
            Ok(bound_id) if bound_id == store_id => Ok(()),
            Ok(bound_id) => {
                self.cleanup_created_marker(&claim).await;
                Err(store_mismatch_error(store_id, &bound_id))
            }
            Err(err) => {
                self.cleanup_created_marker(&claim).await;
                Err(err)
            }
        }
    }

    async fn claim_marker_store_id(&self) -> Result<MarkerClaim> {
        let candidate = new_store_id();
        let marker = serde_json::to_vec(&LayoutMarker {
            layout: LAYOUT_VERSION,
            store_id: Some(candidate.clone()),
        })?;
        if self.root.create_record(ROOT, LAYOUT_FILE, &marker).await? {
            return Ok(MarkerClaim::Created(candidate));
        }
        let store_id = self
            .read_existing_marker_store_id()
            .await?
            .ok_or_else(|| RegistryError::StorageUnavailable {
                reason: format!("the hosted store's {LAYOUT_FILE} has no store_id"),
            })?;
        Ok(MarkerClaim::Existing(store_id))
    }

    async fn cleanup_created_marker(&self, claim: &MarkerClaim) {
        if matches!(claim, MarkerClaim::Created(_)) {
            let _ = self.root.remove_record(ROOT, LAYOUT_FILE).await;
        }
    }

    async fn write_hosted_layout_marker_for(&self, store_id: &str) -> Result<()> {
        let marker = serde_json::to_vec(&LayoutMarker {
            layout: LAYOUT_VERSION,
            store_id: Some(store_id.to_string()),
        })?;
        if self.root.create_record(ROOT, LAYOUT_FILE, &marker).await? {
            return Ok(());
        }
        match self.read_existing_marker_store_id().await? {
            Some(current_id) if current_id == store_id => Ok(()),
            Some(current_id) => Err(store_mismatch_error(&current_id, store_id)),
            None => Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the metadata database is bound to hosted store {store_id}, but the hosted store's {LAYOUT_FILE} has no store_id",
                ),
            }),
        }
    }

    async fn read_existing_marker_store_id(&self) -> Result<Option<String>> {
        let current = self.root.read_record(ROOT, LAYOUT_FILE).await?.unwrap_or_default();
        let current = parse_marker(&current)?;
        if current.layout != LAYOUT_VERSION {
            return Err(other_layout_error(current.layout));
        }
        Ok(current.store_id)
    }

    /// Empty the proxy cache's upstream namespaces when they were written in
    /// another layout than [`LAYOUT_VERSION`]. The cache is re-fetchable, so
    /// it is rebuilt rather than converted.
    pub async fn reset_cache_from_other_layout(&self) -> Result<()> {
        let root = &self.cached.root;
        let marker = root.join(LAYOUT_FILE);
        let current = fs::read(&marker).await
            .ok()
            .and_then(|bytes| parse_marker(&bytes).ok());
        if current.is_some_and(|marker| marker.layout == LAYOUT_VERSION) {
            return Ok(());
        }
        if let Some(mut entries) = crate::read_dir_if_present(root).await? {
            while let Some(entry) = entries.next_entry().await? {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with('~')
                {
                    fs::remove_dir_all(entry.path()).await?;
                }
            }
        }
        fs::create_dir_all(root).await?;
        crate::write_atomic(
            &marker,
            &serde_json::to_vec(&LayoutMarker { layout: LAYOUT_VERSION, store_id: None })?,
        )
        .await
    }
}

/// `(org, ecosystem)`'s namespace of the hosted store `root`.
pub(crate) fn namespace(
    root: &Arc<dyn HostedBackend>,
    org: &str,
    ecosystem: Ecosystem,
) -> Arc<dyn HostedBackend> {
    crate::layout::namespace_segments(org, ecosystem)
        .iter()
        .fold(Arc::clone(root), |backend, segment| backend.namespaced(segment))
        .with_ecosystem(ecosystem)
}

fn parse_marker(bytes: &[u8]) -> Result<LayoutMarker> {
    let marker: LayoutMarker = serde_json::from_slice(bytes)
        .map_err(|err| RegistryError::StorageUnavailable {
            reason: format!("the hosted store's {LAYOUT_FILE} is unreadable: {err}"),
        })?;
    if marker.store_id.as_deref() == Some("") {
        return Err(RegistryError::StorageUnavailable {
            reason: format!("the hosted store's {LAYOUT_FILE} holds an empty store_id"),
        });
    }
    Ok(marker)
}

fn unmarked_with_data_error() -> RegistryError {
    RegistryError::StorageUnavailable {
        reason: format!(
            "the hosted store has no {LAYOUT_FILE} and holds data in an earlier layout; this pnpr reads layout {LAYOUT_VERSION}",
        ),
    }
}

fn verify_store_binding(layout: HostedLayout, binding: StoreBinding) -> Result<HostedLayout> {
    match (&layout, binding) {
        (HostedLayout::Unmarked { has_data: true }, _) => Err(unmarked_with_data_error()),
        (HostedLayout::Other(version), _) => Err(other_layout_error(*version)),
        (HostedLayout::Current { store_id: None }, binding) => {
            Err(missing_marker_id_error(binding))
        }
        (HostedLayout::Current { store_id: Some(store_id) }, StoreBinding::Bound(db_id))
            if *store_id == db_id =>
        {
            Ok(layout)
        }
        (HostedLayout::Current { store_id: Some(store_id) }, StoreBinding::Bound(db_id)) => {
            Err(store_mismatch_error(store_id, &db_id))
        }
        (HostedLayout::Current { store_id: Some(store_id) }, StoreBinding::Unmarked { .. }) => {
            Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the hosted store's {LAYOUT_FILE} is bound to store {store_id}, but the metadata database has no store_id in hosted_store",
                ),
            })
        }
        (HostedLayout::Unmarked { has_data: false }, binding) => {
            verify_unmarked_store(layout, binding)
        }
    }
}

fn missing_marker_id_error(binding: StoreBinding) -> RegistryError {
    let reason = match binding {
        StoreBinding::Bound(db_id) => format!(
            "the metadata database is bound to hosted store {db_id}, but the hosted store's {LAYOUT_FILE} has no store_id",
        ),
        StoreBinding::Unmarked { .. } => {
            format!("the hosted store's {LAYOUT_FILE} has no store_id")
        }
    };
    RegistryError::StorageUnavailable { reason }
}

fn verify_unmarked_store(layout: HostedLayout, binding: StoreBinding) -> Result<HostedLayout> {
    match binding {
        StoreBinding::Bound(db_id) => Err(RegistryError::StorageUnavailable {
            reason: format!(
                "the metadata database is bound to hosted store {db_id}, but the hosted store has no {LAYOUT_FILE}",
            ),
        }),
        StoreBinding::Unmarked { has_data: true } => Err(RegistryError::StorageUnavailable {
            reason: "the metadata database has no store_id in hosted_store and holds data from an earlier schema".to_string(),
        }),
        StoreBinding::Unmarked { has_data: false } => Ok(layout),
    }
}

fn store_mismatch_error(store_id: &str, db_id: &str) -> RegistryError {
    RegistryError::StorageUnavailable {
        reason: format!(
            "the hosted store's {LAYOUT_FILE} is bound to store {store_id}, but the metadata database is bound to store {db_id}",
        ),
    }
}

fn other_layout_error(layout: u32) -> RegistryError {
    let reason = if layout > LAYOUT_VERSION {
        format!(
            "the hosted store uses layout {layout}, written by a newer pnpr; this one reads layout {LAYOUT_VERSION}",
        )
    } else {
        format!("the hosted store uses layout {layout}; this pnpr reads layout {LAYOUT_VERSION}")
    };
    RegistryError::StorageUnavailable { reason }
}
