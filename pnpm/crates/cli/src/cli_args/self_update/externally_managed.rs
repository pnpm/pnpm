use super::SelfUpdateError;
use pnpm_config::standalone_install_command;
use std::path::Path;

/// Refuse to self-update a pnpm whose updates another tool manages: corepack
/// or Homebrew. A switch would install a second pnpm that the one on `PATH`
/// shadows. Checked in the dispatcher *before* project config is loaded, so a
/// broken `.npmrc` / workspace config can't mask the refusal.
pub(crate) fn reject_externally_managed() -> miette::Result<()> {
    if cfg!(target_family = "wasm") {
        return Err(miette::miette!(
            code = "ERR_PNPM_WASM_SELF_UPDATE_UNSUPPORTED",
            "Update this WebAssembly distribution by installing a newer version of @pnpm/wasm"
        ));
    }
    if is_executed_by_corepack() {
        return Err(SelfUpdateError::CantSelfUpdateInCorepack {
            install_command: standalone_install_command(),
        }
        .into());
    }
    if let Some(formula) = std::env::current_exe()
        .and_then(dunce::canonicalize)
        .ok()
        .and_then(|exe| homebrew_formula(&exe))
    {
        return Err(SelfUpdateError::CantSelfUpdateInHomebrew { formula }.into());
    }
    Ok(())
}

/// The Homebrew formula (`pnpm`, `pnpm@11`, ...) whose keg holds `exe`, a
/// canonical path. Every keg lives at `<cellar>/<formula>/<version>/`.
pub(super) fn homebrew_formula(exe: &Path) -> Option<String> {
    let components: Vec<_> = exe.iter().collect();
    components
        .windows(3)
        .find_map(|window| {
            let [cellar, formula, _version] = window else { return None };
            let formula = formula.to_str()?;
            (*cellar == "Cellar" && (formula == "pnpm" || formula.starts_with("pnpm@"))).then(
                || formula.to_string(),
            )
        })
}

/// `true` when pnpm is running under corepack, which manages its own
/// updates (corepack sets `COREPACK_ROOT`).
fn is_executed_by_corepack() -> bool {
    std::env::var_os("COREPACK_ROOT").is_some()
}
