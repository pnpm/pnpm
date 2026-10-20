//! The layout marker of a hosted store, and the proxy cache's
//! wipe-and-rebuild rule.

use crate::{
    HostedBackend, Result, Storage,
    layout::{LAYOUT_FILE, LAYOUT_VERSION, LayoutMarker},
};
use pnpr_error::RegistryError;
use std::sync::Arc;
use tokio::fs;

/// The record namespace the marker is written in: the store root itself.
const ROOT: &str = "";

/// Which layout a hosted store is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedLayout {
    /// [`LAYOUT_VERSION`].
    Current,
    /// Marked with another layout version.
    Other(u32),
    /// No marker. `has_data` says whether anything but shared records is
    /// stored, which would be in the layout from before markers existed.
    Unmarked { has_data: bool },
}

impl Storage {
    /// A view of `org`'s hosted namespace, so two orgs hosting the same
    /// `name@version` can't collide. The empty org is the default namespace,
    /// the one [`Storage::new`] addresses. The disposable proxy cache is
    /// shared (org registries never touch it).
    #[must_use]
    pub fn for_hosted(&self, org: &str) -> Storage {
        Storage {
            root: Arc::clone(&self.root),
            hosted: namespace(&self.root, org),
            cached: self.cached.clone(),
            tar_integrities: Arc::clone(&self.tar_integrities),
            tar_integrity_cache: Arc::clone(&self.tar_integrity_cache),
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
                HostedLayout::Current
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

    /// Check the hosted store's layout before anything writes it, writing the
    /// marker into a store that holds no packages yet.
    ///
    /// # Errors
    ///
    /// See [`Self::check_hosted_layout`].
    pub async fn ensure_hosted_layout(&self) -> Result<()> {
        if self.check_hosted_layout().await? == HostedLayout::Current {
            return Ok(());
        }
        self.write_hosted_layout_marker().await
    }

    /// Check that this pnpr can read the hosted store, without writing to it.
    /// Returns the layout, which is [`HostedLayout::Current`] or an empty
    /// unmarked store.
    ///
    /// # Errors
    ///
    /// A store written by a newer pnpr, or one in an earlier layout, which
    /// `pnpr upgrade-storage` converts.
    pub async fn check_hosted_layout(&self) -> Result<HostedLayout> {
        match self.hosted_layout().await? {
            HostedLayout::Unmarked { has_data: true } => Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the hosted store has no {LAYOUT_FILE} and holds data in the earlier layout. \
                     Back it up and run `pnpr upgrade-storage` to convert it",
                ),
            }),
            HostedLayout::Other(layout) => Err(other_layout_error(layout)),
            layout => Ok(layout),
        }
    }

    /// Mark the hosted store as being in [`LAYOUT_VERSION`]. Only for a store
    /// that is.
    pub async fn write_hosted_layout_marker(&self) -> Result<()> {
        let marker = serde_json::to_vec(&LayoutMarker { layout: LAYOUT_VERSION })?;
        if !self.root.create_record(ROOT, LAYOUT_FILE, &marker).await? {
            let current = self.root.read_record(ROOT, LAYOUT_FILE).await?.unwrap_or_default();
            let layout = parse_marker(&current)?.layout;
            if layout != LAYOUT_VERSION {
                return Err(other_layout_error(layout));
            }
        }
        Ok(())
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
        crate::write_atomic(&marker, &serde_json::to_vec(&LayoutMarker { layout: LAYOUT_VERSION })?)
            .await
    }
}

/// `org`'s namespace of the hosted store `root`.
pub(crate) fn namespace(root: &Arc<dyn HostedBackend>, org: &str) -> Arc<dyn HostedBackend> {
    crate::layout::namespace_segments(org)
        .iter()
        .fold(Arc::clone(root), |backend, segment| backend.namespaced(segment))
}

fn parse_marker(bytes: &[u8]) -> Result<LayoutMarker> {
    serde_json::from_slice(bytes)
        .map_err(|err| RegistryError::StorageUnavailable {
            reason: format!("the hosted store's {LAYOUT_FILE} is unreadable: {err}"),
        })
}

fn other_layout_error(layout: u32) -> RegistryError {
    let reason = if layout > LAYOUT_VERSION {
        format!(
            "the hosted store uses layout {layout}, written by a newer pnpr; this one reads layout {LAYOUT_VERSION}",
        )
    } else {
        format!(
            "the hosted store uses layout {layout}. Back it up and run `pnpr upgrade-storage` to convert it",
        )
    };
    RegistryError::StorageUnavailable { reason }
}
