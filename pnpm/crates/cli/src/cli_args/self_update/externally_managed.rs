use super::SelfUpdateError;
use crate::cli_args::package_manager::wanted_package_manager;
use pnpm_config::standalone_install_command;
use pnpm_package_manifest::PackageManifest;
use std::path::Path;

/// A tool other than pnpm that manages the running pnpm's updates.
#[derive(Debug)]
pub(crate) enum ExternalManager {
    Corepack,
    Homebrew { formula: String },
}

impl ExternalManager {
    /// Reported in place of the global switch after the project pin is
    /// written.
    pub(super) fn global_switch_skipped_message(&self) -> String {
        match self {
            ExternalManager::Corepack => format!(
                "The global pnpm was left alone because it is executed by Corepack. Install pnpm with the standalone script instead: {}",
                standalone_install_command(),
            ),
            ExternalManager::Homebrew { formula } => format!(
                "The global pnpm was left alone because it is installed by Homebrew. Update it with Homebrew instead: brew upgrade {formula}",
            ),
        }
    }
}

impl From<ExternalManager> for SelfUpdateError {
    fn from(manager: ExternalManager) -> Self {
        match manager {
            ExternalManager::Corepack => SelfUpdateError::CantSelfUpdateInCorepack {
                install_command: standalone_install_command(),
            },
            ExternalManager::Homebrew { formula } => {
                SelfUpdateError::CantSelfUpdateInHomebrew { formula }
            }
        }
    }
}

/// The tool that manages the running pnpm's updates, if any.
///
/// A global switch under such a tool would install a second pnpm that the
/// one on `PATH` shadows, so `self-update` only writes the project pin (see
/// [`refuse_without_pin`]).
pub(crate) fn find_external_manager() -> miette::Result<Option<ExternalManager>> {
    if cfg!(target_family = "wasm") {
        return Err(miette::miette!(
            code = "ERR_PNPM_WASM_SELF_UPDATE_UNSUPPORTED",
            "Update this WebAssembly distribution by installing a newer version of @pnpm/wasm"
        ));
    }
    if is_executed_by_corepack() {
        return Ok(Some(ExternalManager::Corepack));
    }
    let formula = std::env::current_exe()
        .and_then(dunce::canonicalize)
        .ok()
        .and_then(|exe| homebrew_formula(&exe));
    Ok(formula.map(|formula| ExternalManager::Homebrew { formula }))
}

/// Fail under an external `manager` when the project's pin `manifest` does
/// not pin pnpm: the global switch is all that would be left to do. A
/// manifest that cannot be read holds no pin the update could write.
pub(super) fn refuse_without_pin(
    manager: Option<ExternalManager>,
    manifest: Option<&Option<PackageManifest>>,
) -> miette::Result<Option<ExternalManager>> {
    let Some(manager) = manager else {
        return Ok(None);
    };
    let pins_pnpm = manifest
        .and_then(Option::as_ref)
        .and_then(|manifest| wanted_package_manager(manifest.value()))
        .is_some_and(|pm| pm.name == "pnpm");
    if pins_pnpm {
        return Ok(Some(manager));
    }
    Err(SelfUpdateError::from(manager).into())
}

/// The Homebrew formula (`pnpm`, `pnpm@11`, ...) whose keg holds `exe`, a
/// canonical path. A keg is `<cellar>/<formula>/<version>/`, and Homebrew
/// writes `INSTALL_RECEIPT.json` into every keg it installs.
pub(super) fn homebrew_formula(exe: &Path) -> Option<String> {
    exe.ancestors()
        .find_map(|keg| {
            let formula_dir = keg.parent()?;
            let formula = formula_dir.file_name()?.to_str()?;
            let is_pnpm_keg = formula_dir.parent()?.file_name()? == "Cellar"
                && (formula == "pnpm" || formula.starts_with("pnpm@"))
                && keg.join("INSTALL_RECEIPT.json").is_file();
            is_pnpm_keg.then(|| formula.to_string())
        })
}

/// `true` when pnpm is running under corepack, which manages its own
/// updates (corepack sets `COREPACK_ROOT`).
fn is_executed_by_corepack() -> bool {
    std::env::var_os("COREPACK_ROOT").is_some()
}
