use super::SelfUpdateError;
use crate::cli_args::package_manager::{read_manifest_json, wanted_package_manager};
use pnpm_config::standalone_install_command;
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
/// one on `PATH` shadows, so `self-update` only writes the project pin of
/// `dir`. Without a pnpm pin there is nothing left to do, and this fails.
///
/// Checked in the dispatcher *before* project config is loaded, so a broken
/// `.npmrc` / workspace config can't mask the refusal.
pub(crate) fn check_externally_managed(dir: &Path) -> miette::Result<Option<ExternalManager>> {
    if cfg!(target_family = "wasm") {
        return Err(miette::miette!(
            code = "ERR_PNPM_WASM_SELF_UPDATE_UNSUPPORTED",
            "Update this WebAssembly distribution by installing a newer version of @pnpm/wasm"
        ));
    }
    let Some(manager) = find_external_manager() else {
        return Ok(None);
    };
    if project_pins_pnpm(dir) {
        return Ok(Some(manager));
    }
    Err(SelfUpdateError::from(manager).into())
}

fn find_external_manager() -> Option<ExternalManager> {
    if is_executed_by_corepack() {
        return Some(ExternalManager::Corepack);
    }
    let formula = std::env::current_exe()
        .and_then(dunce::canonicalize)
        .ok()
        .and_then(|exe| homebrew_formula(&exe))?;
    Some(ExternalManager::Homebrew { formula })
}

/// A manifest that cannot be read holds no pin the update could write.
fn project_pins_pnpm(dir: &Path) -> bool {
    read_manifest_json(&dir.join("package.json"))
        .ok()
        .flatten()
        .as_ref()
        .and_then(wanted_package_manager)
        .is_some_and(|pm| pm.name == "pnpm")
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
