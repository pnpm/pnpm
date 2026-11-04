#[derive(Debug, Clone, clap::Args)]
pub struct PublishRegistryArgs {
    /// Register the published package under this tag instead of `latest`.
    #[clap(long)]
    pub tag: Option<String>,
    /// Publish the package as `public` or `restricted`.
    #[clap(long, value_parser = ["public", "restricted"])]
    pub access: Option<String>,
    /// Generate a provenance attestation for the published package.
    #[clap(long, overrides_with = "no_provenance")]
    pub provenance: bool,
    /// Do not generate a provenance attestation, even under trusted publishing.
    #[clap(long = "no-provenance", hide = true, overrides_with = "provenance")]
    pub no_provenance: bool,
    /// One-time password for two-factor-authenticated registries.
    #[clap(long)]
    pub otp: Option<String>,
    /// Wait up to this many milliseconds per package for registry availability.
    /// A batch upload uses one timeout per registry group. Zero disables waiting (default).
    #[clap(long, value_name = "MILLISECONDS")]
    pub publish_wait_timeout: Option<u64>,
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
    #[clap(flatten)]
    pub notes: PublishNotesArgs,
}

/// How the archives a package's `publishConfig.variants` declares are
/// published.
#[derive(Debug, Default, Clone, clap::Args)]
pub struct PublishVariantArgs {
    /// Publish only the archive that targets `any` of a package that declares
    /// `publishConfig.variants`, such as to a registry that does not serve
    /// platform variants.
    #[clap(long)]
    pub no_variants: bool,
    /// Pack the archive of the `publishConfig.variants` entry this target
    /// names, such as `linux-x64-musl` or `any`, upload it to the registry's
    /// staging area, and print `<target>=<integrity>`. Publishes nothing. Can
    /// be repeated.
    #[clap(long = "upload-variant", value_name = "TARGET")]
    pub upload_variant: Vec<String>,
    /// Publish the archive another job uploaded with `--upload-variant`, as
    /// `<target>=<integrity>`, or `<name>~<target>=<integrity>` with
    /// `--recursive`. Repeated once per archive.
    #[clap(long = "variant-integrity", value_name = "TARGET=INTEGRITY")]
    pub variant_integrity: Vec<String>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct PublishGitArgs {
    /// The branch publishing is allowed from. Overrides the `publishBranch`
    /// setting. Without either, defaults to `master` / `main`.
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

#[derive(Debug, Clone, clap::Args)]
pub struct PublishNotesArgs {
    /// A file of release notes to send to a registry declared with
    /// `serverType: pnpr`. Publishing one package, they are its version
    /// notes. Publishing several with `--batch`, they are the notes of the
    /// whole release.
    #[clap(long = "release-notes", value_name = "FILE")]
    pub release_notes: Option<String>,
    /// Send no release notes, not even those read from `CHANGELOG.md` or
    /// change intents.
    #[clap(long = "no-release-notes", conflicts_with = "release_notes")]
    pub no_release_notes: bool,
    /// The name of the release that `--batch` publishes to a registry
    /// declared with `serverType: pnpr`, such as `v12.0.0`.
    #[clap(long = "publication-name", value_name = "NAME")]
    pub publication_name: Option<String>,
}
