use clap::Args;
use pacquet_package_is_installable::SupportedArchitectures;

/// `--cpu`, `--libc`, `--os` CLI flags. Multi-valued: each may be
/// repeated (`--cpu arm64 --cpu x64`) or comma-separated
/// (`--cpu arm64,x64`) — both shapes are wired so the surface
/// matches every reasonable user expectation.
///
/// Flattened into the `InstallArgs` / `AddArgs` clap derives so the
/// three flags appear under the regular `--help` output. Shared
/// between the two so the wire shape is identical.
#[derive(Debug, Default, Clone, Args)]
pub struct SupportedArchitecturesArgs {
    /// CPU architectures whose platform-tagged optional dependencies
    /// should be kept. Repeat or comma-separate for multiple values.
    /// Overrides `supportedArchitectures.cpu` from
    /// `pnpm-workspace.yaml` for this axis only.
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub cpu: Vec<String>,

    /// Operating systems whose platform-tagged optional dependencies
    /// should be kept. Overrides `supportedArchitectures.os`.
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub os: Vec<String>,

    /// libc families whose platform-tagged optional dependencies
    /// should be kept (`glibc`, `musl`). Overrides
    /// `supportedArchitectures.libc`.
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub libc: Vec<String>,
}

impl SupportedArchitecturesArgs {
    /// Apply the CLI overrides to a config-derived `SupportedArchitectures`
    /// value, returning the merged result. Returns `None` only when both
    /// the existing config value and every CLI axis are empty.
    pub fn apply_to(
        &self,
        existing: Option<SupportedArchitectures>,
    ) -> Option<SupportedArchitectures> {
        if self.cpu.is_empty() && self.os.is_empty() && self.libc.is_empty() {
            return existing;
        }
        let mut out = existing.unwrap_or_default();
        if !self.cpu.is_empty() {
            out.cpu = Some(self.cpu.clone());
        }
        if !self.os.is_empty() {
            out.os = Some(self.os.clone());
        }
        if !self.libc.is_empty() {
            out.libc = Some(self.libc.clone());
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests;
