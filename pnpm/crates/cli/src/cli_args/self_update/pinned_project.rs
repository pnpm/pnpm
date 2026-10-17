use super::{
    Config, Context, ExternalManager, PNPM_VERSION, Path,
    externally_managed::refuse_without_pin,
    project_pin::{project_pin_refusal, read_project_pinned_pnpm_version, update_project_pin},
};
use crate::cli_args::package_manager::{PinRoots, WantedPackageManager, wanted_package_manager};

/// The project's package-manager pin, read from where the pre-command pass
/// reads it (see [`PinRoots`]).
pub(super) struct ProjectPin {
    roots: PinRoots,
    wanted: Option<WantedPackageManager>,
}

impl ProjectPin {
    /// Read the pin of the project `dir` belongs to. Fails under an
    /// `external_manager` when the project does not pin pnpm (see
    /// [`refuse_without_pin`]).
    pub(super) fn read(
        config: &Config,
        dir: &Path,
        external_manager: Option<ExternalManager>,
    ) -> miette::Result<(Self, Option<ExternalManager>)> {
        let roots = PinRoots::of(config, dir);
        let manifest = roots.read_manifest();
        let external_manager = refuse_without_pin(external_manager, manifest.as_ref().ok())?;
        let manifest = manifest.wrap_err("read the project manifest")?;
        let wanted =
            manifest.as_ref().and_then(|manifest| wanted_package_manager(manifest.value()));
        Ok((Self { roots, wanted }, external_manager))
    }

    fn pinned_pnpm(&self) -> Option<&WantedPackageManager> {
        self.wanted
            .as_ref()
            .filter(|pm| pm.name == "pnpm")
    }

    /// The migration hint for the major boundary this update crosses, if it
    /// crosses one. The project's pin is the source of truth for the version
    /// being left behind; the running binary stands in when nothing pins
    /// pnpm.
    pub(super) fn crossed_major_hint(&self, target_version: &str) -> Option<&'static str> {
        let previous_version = match self.pinned_pnpm() {
            // A range pin (`^10.0.0`) has no recoverable major on its own, so
            // read the resolved version from the env lockfile to drive the
            // hint — otherwise crossing a major would silently skip it.
            Some(pm) => read_project_pinned_pnpm_version(&self.roots.env, pm.version.as_deref())
                .filter(|version| version != target_version),
            None if PNPM_VERSION != target_version => Some(PNPM_VERSION.to_string()),
            None => None,
        }?;
        let previous_major = node_semver::Version::parse(&previous_version).ok()?.major;
        let target = node_semver::Version::parse(target_version).ok()?;
        (target.major > previous_major).then(|| major_upgrade_hint(target.major))?
    }

    /// See [`project_pin_refusal`]. `None` when the project does not pin
    /// pnpm.
    pub(super) async fn refusal(
        &self,
        config: &'static Config,
        target_version: &str,
        is_implicit_latest: bool,
    ) -> miette::Result<Option<String>> {
        let Some(pm) = self.pinned_pnpm() else {
            return Ok(None);
        };
        Box::pin(project_pin_refusal(
            config,
            &self.roots.env,
            pm,
            target_version,
            is_implicit_latest,
        ))
        .await
    }

    /// Point a pnpm pin at `target_version`, returning the message that
    /// reports it. `None` when the project does not pin pnpm.
    ///
    /// The manifest and its pin are read again, since the global install
    /// that runs before this can take long enough for the user to edit them.
    pub(super) async fn update(
        self,
        config: &'static Config,
        target_version: &str,
    ) -> miette::Result<Option<String>> {
        if self.pinned_pnpm().is_none() {
            return Ok(None);
        }
        let Some(manifest) = self.roots.read_manifest().wrap_err("read the project manifest")?
        else {
            return Ok(None);
        };
        let Some(pm) = wanted_package_manager(manifest.value()).filter(|pm| pm.name == "pnpm")
        else {
            return Ok(None);
        };
        Box::pin(update_project_pin(config, &self.roots.env, manifest, &pm, target_version))
            .await
            .map(Some)
    }
}

/// Migration guidance printed once when `self-update` crosses a major
/// boundary. Add an entry per future major that ships breaking changes
/// users need to act on.
fn major_upgrade_hint(target_major: u64) -> Option<&'static str> {
    match target_major {
        11 => Some(
            "pnpm v11 removed or renamed several v10 settings. \
             See https://pnpm.io/11.x/migration for migration instructions.",
        ),
        _ => None,
    }
}
