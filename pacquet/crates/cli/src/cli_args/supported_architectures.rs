use clap::Args;
use pacquet_package_is_installable::SupportedArchitectures;

/// `--cpu`, `--libc`, `--os` CLI flags. Multi-valued: each may be
/// repeated (`--cpu arm64 --cpu x64`) or comma-separated
/// (`--cpu arm64,x64`) — both shapes are wired so the surface
/// matches every reasonable user expectation.
///
/// Each axis present on the command line REPLACES that axis in the
/// `config.supported_architectures` value derived from
/// `pnpm-workspace.yaml`. Absent axes leave their config-supplied
/// value untouched. Mirrors upstream's
/// [`overrideSupportedArchitecturesWithCLI`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/overrideSupportedArchitecturesWithCLI.ts):
///
/// ```ts
/// for (const key of CLI_OPTION_NAMES) {
///   const values = cliOptions[key]
///   if (values != null) {
///     targetConfig.supportedArchitectures ??= {}
///     targetConfig.supportedArchitectures[key] = typeof values === 'string' ? [values] : values
///   }
/// }
/// ```
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
    /// value, returning the merged result. An axis present on the
    /// command line replaces the corresponding `existing` axis;
    /// absent axes pass through unchanged.
    ///
    /// Returns `None` only when both the existing config value and
    /// every CLI axis are empty — same shape as
    /// `targetConfig.supportedArchitectures` being left `undefined`
    /// upstream.
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
mod tests {
    use super::SupportedArchitecturesArgs;
    use pacquet_package_is_installable::SupportedArchitectures;
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_cli_passes_existing_through() {
        let cli = SupportedArchitecturesArgs::default();
        let existing = Some(SupportedArchitectures {
            os: Some(vec!["darwin".to_string()]),
            cpu: None,
            libc: None,
        });
        assert_eq!(cli.apply_to(existing.clone()), existing);
    }

    /// CLI overrides individual axes wholesale; other axes survive
    /// from config. Mirrors upstream's `targetConfig.supportedArchitectures[key] = values`
    /// per-axis assignment.
    #[test]
    fn cli_cpu_replaces_config_cpu_only() {
        let cli =
            SupportedArchitecturesArgs { cpu: vec!["x64".to_string()], os: vec![], libc: vec![] };
        let existing = Some(SupportedArchitectures {
            os: Some(vec!["darwin".to_string()]),
            cpu: Some(vec!["arm64".to_string()]),
            libc: None,
        });
        let merged = cli.apply_to(existing).unwrap();
        assert_eq!(merged.os, Some(vec!["darwin".to_string()]));
        assert_eq!(merged.cpu, Some(vec!["x64".to_string()]));
        assert_eq!(merged.libc, None);
    }

    /// CLI without an existing config value still produces a
    /// populated `SupportedArchitectures` — equivalent to upstream's
    /// `targetConfig.supportedArchitectures ??= {}` then per-axis
    /// assignment.
    #[test]
    fn cli_without_existing_creates_supported_architectures() {
        let cli = SupportedArchitecturesArgs {
            cpu: vec!["x64".to_string()],
            os: vec!["linux".to_string()],
            libc: vec!["glibc".to_string()],
        };
        let merged = cli.apply_to(None).unwrap();
        assert_eq!(merged.cpu, Some(vec!["x64".to_string()]));
        assert_eq!(merged.os, Some(vec!["linux".to_string()]));
        assert_eq!(merged.libc, Some(vec!["glibc".to_string()]));
    }
}
