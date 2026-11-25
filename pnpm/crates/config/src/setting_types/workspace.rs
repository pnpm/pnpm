pub use pnpm_workspace::map_settings::CatalogMode;

use super::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageImportMethod {
    /// Try the platform's cheap link tiers in order (hardlink first on
    /// Linux and Windows, clone first elsewhere) and fall back to copying
    /// when none is possible. `deps-restorer::link_file::next_auto_tier`
    /// implements the ladder and carries the rationale.
    #[default]
    Auto,

    /// hard link packages from the store
    Hardlink,

    /// copy packages from the store
    Copy,

    /// clone (AKA copy-on-write or reference link) packages from the store
    Clone,

    /// try to clone packages from the store. If cloning is not supported then fall back to copying
    CloneOrCopy,
}
