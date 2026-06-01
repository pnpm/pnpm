use crate::{Install, InstallError, ResolvedPackages};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_tarball::MemCache;
use std::sync::Arc;

/// This subroutine does everything `pacquet remove` is supposed to do.
#[must_use]
pub struct Remove<'a> {
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub http_client_arc: Arc<ThrottledClient>,
    pub config: &'static Config,
    pub manifest: &'a mut PackageManifest,
    pub lockfile: Option<&'a Lockfile>,
    pub lockfile_path: Option<&'a std::path::Path>,
    /// Names to remove. Empty is rejected with
    /// [`RemoveError::MustRemoveSomething`].
    pub package_names: &'a [String],
    /// Dependency field to restrict removal to, or `None` to remove from
    /// any field. Derived from the `--save-prod` / `--save-dev` /
    /// `--save-optional` flags via pnpm's `getSaveType`.
    pub save_type: Option<DependencyGroup>,
    /// CLI-merged `supportedArchitectures` forwarded to the follow-up
    /// `Install` run. See [`Install::supported_architectures`].
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    /// `--lockfile-only`: rewrite `pnpm-lock.yaml` (and the manifest) but
    /// skip materializing `node_modules`. Forwarded to the follow-up
    /// `Install` run. See [`Install::lockfile_only`].
    pub lockfile_only: bool,
}

/// Error type of [`Remove`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum RemoveError {
    /// `pacquet remove` was invoked with no package names. Mirrors pnpm's
    /// `ERR_PNPM_MUST_REMOVE_SOMETHING` thrown at
    /// <https://github.com/pnpm/pnpm/blob/9cad8274fd/installing/commands/src/remove.ts>.
    #[display("At least one dependency name should be specified for removal")]
    #[diagnostic(code(ERR_PNPM_MUST_REMOVE_SOMETHING))]
    MustRemoveSomething,

    /// One or more names passed to `pacquet remove` aren't present in the
    /// targeted dependency field(s). Mirrors pnpm's
    /// `ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS`; the message and hint are
    /// built to match upstream's `RemoveMissingDepsError`.
    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS))]
    CannotRemoveMissingDeps {
        message: String,
        #[help]
        hint: Option<String>,
    },

    #[display("Failed save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),
}

impl<'a> Remove<'a> {
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), RemoveError> {
        let Remove {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            package_names,
            save_type,
            resolved_packages,
            supported_architectures,
            lockfile_only,
        } = self;

        if package_names.is_empty() {
            return Err(RemoveError::MustRemoveSomething);
        }

        let available_dependencies = manifest.available_dependency_names(save_type);
        let non_matched_dependencies: Vec<&String> = package_names
            .iter()
            .filter(|name| !available_dependencies.iter().any(|dep| dep == *name))
            .collect();
        if !non_matched_dependencies.is_empty() {
            return Err(cannot_remove_missing_deps(
                &available_dependencies,
                &non_matched_dependencies,
                save_type,
            ));
        }

        manifest.remove_dependencies(package_names, save_type);

        Install {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            // `pnpm remove`'s `include` defaults to every dependency
            // group (`production`/`dev`/`optional` !== false), so the
            // re-resolve walks all three. Mirrors upstream's `include`
            // at <https://github.com/pnpm/pnpm/blob/9cad8274fd/installing/commands/src/remove.ts>.
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            frozen_lockfile: false,
            // `pacquet remove` mutates the manifest, so the lockfile is
            // necessarily stale — short-circuit the prefer-frozen fast
            // path so the install always re-resolves. See the parallel
            // comment in `add.rs`.
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            // `pacquet remove` is a partial install (pnpm's
            // `mutation: 'uninstallSome'`), so the root project's own
            // lifecycle scripts must not run — mirroring pnpm's
            // `mutation === 'install'` filter.
            is_full_install: false,
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
        }
        .run::<Reporter>()
        .await
        .map_err(RemoveError::Install)?;

        manifest.save().map_err(RemoveError::SaveManifest)?;

        // `pnpm:package-manifest updated` mirrors the post-mutation emit
        // pnpm fires after rewriting the manifest. See the parallel emit
        // in `add.rs` for the `prefix` derivation rationale.
        let prefix = manifest
            .path()
            .parent()
            .unwrap_or_else(|| manifest.path())
            .to_string_lossy()
            .into_owned();
        Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
            level: LogLevel::Debug,
            message: PackageManifestMessage::Updated { prefix, updated: manifest.value().clone() },
        }));

        Ok(())
    }
}

/// Build the `ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS` error, mirroring
/// upstream's `RemoveMissingDepsError` message and hint at
/// <https://github.com/pnpm/pnpm/blob/9cad8274fd/installing/commands/src/remove.ts>.
fn cannot_remove_missing_deps(
    available_dependencies: &[String],
    non_matched_dependencies: &[&String],
    target_dependencies_field: Option<DependencyGroup>,
) -> RemoveError {
    let quoted = non_matched_dependencies
        .iter()
        .map(|dep| format!("'{dep}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut message = format!("Cannot remove {quoted}: ");
    if available_dependencies.is_empty() {
        match target_dependencies_field {
            Some(field) => {
                message.push_str(&format!("project has no '{}'", <&str>::from(field)));
            }
            None => message.push_str("project has no dependencies of any kind"),
        }
        return RemoveError::CannotRemoveMissingDeps { message, hint: None };
    }
    let noun = if non_matched_dependencies.len() > 1 { "dependencies" } else { "dependency" };
    let in_field = target_dependencies_field
        .map(|field| format!(" in '{}'", <&str>::from(field)))
        .unwrap_or_default();
    message.push_str(&format!("no such {noun} found{in_field}"));
    let hint = format!("Available dependencies: {}", available_dependencies.join(", "));
    RemoveError::CannotRemoveMissingDeps { message, hint: Some(hint) }
}
