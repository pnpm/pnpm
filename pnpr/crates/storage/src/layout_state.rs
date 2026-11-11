//! The layout marker of a hosted store, and the proxy cache's
//! wipe-and-rebuild rule.

use crate::{
    HostedBackend, Result, Storage,
    layout::{LAYOUT_FILE, LAYOUT_VERSION, LayoutMarker, new_store_id},
    metadata::sql::{StoreBinding, StoreClaim},
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

impl Storage {
    /// A view of `(namespace, ecosystem)` in the hosted store, so two
    /// registries hosting the same `name@version` in separate namespaces
    /// cannot collide. The empty string is the default namespace, and
    /// `("", Ecosystem::Npm)` is the one [`Storage::new`] addresses. The
    /// disposable proxy cache is shared.
    #[must_use]
    pub fn for_hosted(&self, namespace: &str, ecosystem: Ecosystem) -> Storage {
        let hosted = slice(&self.root, namespace, ecosystem);
        Storage {
            root: Arc::clone(&self.root),
            hosted,
            cached: self.cached.clone(),
            tar_integrities: Arc::clone(&self.tar_integrities),
            tar_integrity_cache: Arc::clone(&self.tar_integrity_cache),
            database: Arc::clone(&self.database),
            metadata: self.database.view(namespace, ecosystem),
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
        let layout = self.hosted_layout().await?;
        let binding = self.database.store_binding().await?;
        match (&layout, &binding) {
            (
                HostedLayout::Current { store_id: Some(store_id) },
                StoreBinding::Pending(pending_nonce),
            ) if marker_matches_pending_claim(store_id, pending_nonce) => {
                return self.confirm_claimed_marker(pending_nonce, store_id).await;
            }
            (HostedLayout::Unmarked { has_data: false }, StoreBinding::Pending(_)) => {
                return self.claim_and_write_layout_marker().await;
            }
            _ => {}
        }
        if matches!(
            verify_store_binding(layout, binding)?,
            HostedLayout::Current { store_id: Some(_) },
        ) {
            return Ok(());
        }
        self.claim_and_write_layout_marker().await
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
        self.ensure_hosted_layout().await
    }

    pub(crate) async fn claim_and_write_layout_marker(&self) -> Result<()> {
        let candidate_nonce = new_store_id();
        match self.database.begin_store_claim(candidate_nonce).await? {
            StoreClaim::Pending(pending_nonce) => {
                self.write_or_confirm_pending_marker(&pending_nonce).await
            }
            StoreClaim::Bound(bound_id) => self.verify_bound_store(&bound_id).await,
        }
    }

    async fn write_or_confirm_pending_marker(&self, pending_nonce: &str) -> Result<()> {
        let candidate = claimed_store_id(pending_nonce);
        let marker = serde_json::to_vec(&LayoutMarker {
            layout: LAYOUT_VERSION,
            store_id: Some(candidate.clone()),
        })?;
        if self.root.create_record(ROOT, LAYOUT_FILE, &marker).await? {
            return self.confirm_claimed_marker(pending_nonce, &candidate).await;
        }
        let existing_id = self.require_existing_marker_store_id().await?;
        if marker_matches_pending_claim(&existing_id, pending_nonce) {
            return self.confirm_claimed_marker(pending_nonce, &existing_id).await;
        }
        Err(unmarked_database_error(&existing_id))
    }

    async fn confirm_claimed_marker(&self, pending_nonce: &str, store_id: &str) -> Result<()> {
        match self.database.confirm_store_claim(pending_nonce.to_string(), store_id.to_string())
            .await?
        {
            StoreBinding::Bound(bound_id) if bound_id == store_id => Ok(()),
            StoreBinding::Bound(bound_id) => {
                self.remove_unclaimed_marker(store_id).await;
                Err(store_mismatch_error(store_id, &bound_id))
            }
            StoreBinding::Pending(other_nonce) => Err(store_mismatch_error(store_id, &other_nonce)),
            StoreBinding::Unmarked { .. } => Err(unmarked_database_error(store_id)),
        }
    }

    async fn verify_bound_store(&self, bound_id: &str) -> Result<()> {
        let layout = self.hosted_layout().await?;
        verify_store_binding(layout, StoreBinding::Bound(bound_id.to_string())).map(|_| ())
    }

    async fn remove_unclaimed_marker(&self, store_id: &str) {
        if self
            .read_existing_marker_store_id()
            .await
            .ok()
            .flatten()
            .as_deref()
            == Some(store_id)
        {
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

    async fn require_existing_marker_store_id(&self) -> Result<String> {
        self.read_existing_marker_store_id().await?
            .ok_or_else(|| RegistryError::StorageUnavailable {
                reason: format!("the hosted store's {LAYOUT_FILE} has no store_id"),
            })
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

/// `(namespace, ecosystem)`'s slice of the hosted store `root`.
pub(crate) fn slice(
    root: &Arc<dyn HostedBackend>,
    namespace: &str,
    ecosystem: Ecosystem,
) -> Arc<dyn HostedBackend> {
    crate::layout::namespace_segments(namespace, ecosystem)
        .iter()
        .fold(Arc::clone(root), |backend, segment| backend.namespaced(segment))
        .with_ecosystem(ecosystem)
}

const PENDING_PREFIX_HEX_LEN: usize = 16;

fn claimed_store_id(pending_nonce: &str) -> String {
    let prefix = &pending_nonce[..pending_nonce.len().min(PENDING_PREFIX_HEX_LEN)];
    let suffix = new_store_id();
    format!("{prefix}{}", &suffix[..PENDING_PREFIX_HEX_LEN])
}

fn marker_matches_pending_claim(store_id: &str, pending_nonce: &str) -> bool {
    pending_nonce.len() >= PENDING_PREFIX_HEX_LEN
        && store_id.len() == PENDING_PREFIX_HEX_LEN * 2
        && store_id.starts_with(&pending_nonce[..PENDING_PREFIX_HEX_LEN])
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
        (HostedLayout::Current { store_id: Some(store_id) }, StoreBinding::Pending(db_id)) => {
            Err(pending_binding_error(store_id, &db_id))
        }
        (HostedLayout::Current { store_id: Some(store_id) }, StoreBinding::Unmarked { .. }) => {
            Err(unmarked_database_error(store_id))
        }
        (HostedLayout::Unmarked { has_data: false }, binding) => {
            verify_unmarked_store(layout, binding)
        }
    }
}

fn missing_marker_id_error(binding: StoreBinding) -> RegistryError {
    let reason = match binding {
        StoreBinding::Bound(db_id) | StoreBinding::Pending(db_id) => format!(
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
        StoreBinding::Bound(db_id) | StoreBinding::Pending(db_id) => {
            Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the metadata database is bound to hosted store {db_id}, but the hosted store has no {LAYOUT_FILE}",
                ),
            })
        }
        StoreBinding::Unmarked { has_data: true } => Err(unmarked_database_with_data_error()),
        StoreBinding::Unmarked { has_data: false } => Ok(layout),
    }
}

fn pending_binding_error(store_id: &str, pending_nonce: &str) -> RegistryError {
    if marker_matches_pending_claim(store_id, pending_nonce) {
        return RegistryError::StorageUnavailable {
            reason: format!(
                "the metadata database has an unconfirmed binding to hosted store {store_id}",
            ),
        };
    }
    store_mismatch_error(store_id, pending_nonce)
}

fn unmarked_database_error(store_id: &str) -> RegistryError {
    RegistryError::StorageUnavailable {
        reason: format!(
            "the hosted store's {LAYOUT_FILE} is bound to store {store_id}, but the metadata database has no store_id in hosted_store",
        ),
    }
}

fn unmarked_database_with_data_error() -> RegistryError {
    RegistryError::StorageUnavailable {
        reason: "the metadata database has no store_id in hosted_store and holds data from an earlier schema".to_string(),
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
