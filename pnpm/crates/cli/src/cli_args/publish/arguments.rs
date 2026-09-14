#[derive(Debug, Clone, clap::Args)]
pub struct PublishRegistryArgs {
    /// Register the published package under this tag instead of `latest`.
    #[clap(long)]
    pub tag: Option<String>,
    /// Publish the package as `public` or `restricted`.
    #[clap(long, value_parser = ["public", "restricted"])]
    pub access: Option<String>,
    /// Generate a provenance attestation for the published package.
    #[clap(long)]
    pub provenance: bool,
    /// One-time password for two-factor-authenticated registries.
    #[clap(long)]
    pub otp: Option<String>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct PublishManifestArgs {
    /// Embed the README contents in the published manifest.
    #[clap(long = "embed-readme", overrides_with = "no_embed_readme")]
    pub embed_readme: bool,
    /// Do not embed README contents in the published manifest.
    #[clap(long = "no-embed-readme", hide = true, overrides_with = "embed_readme")]
    pub no_embed_readme: bool,
    /// Keep the original `packageManager` field and publish-lifecycle scripts
    /// in the published manifest instead of stripping them.
    #[clap(long = "skip-manifest-obfuscation", overrides_with = "no_skip_manifest_obfuscation")]
    pub skip_manifest_obfuscation: bool,
    /// Apply pnpm's normal published-manifest filtering.
    #[clap(
        long = "no-skip-manifest-obfuscation",
        hide = true,
        overrides_with = "skip_manifest_obfuscation"
    )]
    pub no_skip_manifest_obfuscation: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct PublishGitArgs {
    /// The branch publishing is allowed from. Defaults to `master` / `main`.
    #[clap(long = "publish-branch")]
    pub publish_branch: Option<String>,
    /// Skip the git working-tree / branch / remote checks.
    #[clap(long = "no-git-checks")]
    pub no_git_checks: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct PublishOutputArgs {
    /// Print the per-package publish summary in JSON.
    #[clap(long)]
    pub json: bool,
    /// Recursive only: write a `pnpm-publish-summary.json` report listing the
    /// packages that were published.
    #[clap(long = "report-summary")]
    pub report_summary: bool,
}
