use crate::shim::CmdShimBatch;

/// The Windows shims a package's bins get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowsShimPolicy {
    /// Whether a `.ps1` shim sits next to the `.cmd` one. See
    /// [`wants_powershell_shim`].
    pub(super) powershell: bool,
    /// See [`cmd_shim_batch`].
    pub(super) cmd_batch: CmdShimBatch,
}

impl WindowsShimPolicy {
    pub(super) fn for_package(pkg_name: &str) -> Self {
        WindowsShimPolicy {
            powershell: wants_powershell_shim(pkg_name),
            cmd_batch: cmd_shim_batch(pkg_name),
        }
    }
}

/// Whether `pkg_name` is the pnpm CLI itself. `@pnpm/exe` is that same CLI
/// under the name earlier installs used.
fn is_pnpm_cli(pkg_name: &str) -> bool {
    matches!(pkg_name, "pnpm" | "@pnpm/exe")
}

/// Whether the bins of `pkg_name` get a PowerShell shim next to the `.cmd`
/// one. The pnpm CLI opts out, because PowerShell resolves `pnpm.ps1` ahead of
/// `pnpm.cmd`: a shim written for one installation of the CLI would keep
/// shadowing every later one, including an upgrade that ships a different
/// executable.
pub(super) fn wants_powershell_shim(pkg_name: &str) -> bool {
    !is_pnpm_cli(pkg_name)
}

/// Whether the `.cmd` shims of `pkg_name` keep their batch context while the
/// target runs. The pnpm CLI's end it: without a `.ps1` (see
/// [`wants_powershell_shim`]), PowerShell and cmd.exe both run the CLI through
/// its `.cmd` shim, so every Ctrl+C that stops a script `pnpm` runs would
/// otherwise end in `Terminate batch job (Y/N)?`.
pub(super) fn cmd_shim_batch(pkg_name: &str) -> CmdShimBatch {
    if is_pnpm_cli(pkg_name) { CmdShimBatch::EndedBeforeTarget } else { CmdShimBatch::Kept }
}
