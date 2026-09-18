use super::{
    super::{
        build::{self, Buildable, Contract},
        host,
        registry::Registry,
    },
    read_manifest,
};
use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::PackageName;
use pnpm_python_resolver::{Candidate, LockedVcs, parse_requirement};
use pnpm_reporter::Reporter;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

impl Registry<'_> {
    pub(super) async fn fetch_git<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        mut vcs: LockedVcs,
    ) -> Result<()> {
        let approval = parse_requirement(name.as_ref())?;
        let unapproved = build::unapproved(self.config, &[approval]);
        if !unapproved.is_empty() {
            bail!(
                "building Python git dependency {name} requires approval of pkg:pypi/{name} under allowBuilds",
            );
        }
        let (checkout, commit) = checkout(self.config, &vcs).await?;
        vcs.commit_id = commit;
        let root = project_root(checkout.path(), vcs.subdirectory.as_deref())?;
        let manifest = read_manifest(&root).await?;
        let built = Box::pin(self.sources.prepare.build::<Reporter>(Buildable {
            root: &root,
            manifest: &manifest,
            editable: false,
            contract: Contract::ResolutionSource,
        }))
        .await?;
        let build::Build::Made(mut built) = built else {
            bail!(
                "the build requirements of Python git dependency {name} are not approved under allowBuilds",
            );
        };
        if built.wheel.metadata.name.parse::<PackageName>().into_diagnostic()? != *name {
            bail!("Python git dependency {name} built a wheel of {}", built.wheel.metadata.name);
        }
        let version: pep440_rs::Version = built.wheel.metadata.version.parse().into_diagnostic()?;
        self.check_source_wheel(&built.wheel, name, &version)?;
        built.wheel.direct_url = Some(host::DirectUrl::git(&vcs));
        self.resolution.packages.candidates.insert(
            name.clone(),
            BTreeMap::from([(version.clone(), Candidate::Vcs(vcs))]),
        );
        self.remember(name.clone(), version.clone(), built.wheel.clone());
        self.sources.built.insert((name.clone(), version), *built);
        Ok(())
    }
}

fn project_root(checkout: &Path, subdirectory: Option<&str>) -> Result<PathBuf> {
    let root = subdirectory.map_or_else(|| checkout.to_path_buf(), |path| checkout.join(path));
    let canonical = dunce::canonicalize(&root).into_diagnostic()?;
    if !canonical.starts_with(dunce::canonicalize(checkout).into_diagnostic()?) {
        bail!("Python git subdirectory escapes its repository");
    }
    Ok(canonical)
}

async fn checkout(
    config: &'static pnpm_config::Config,
    vcs: &LockedVcs,
) -> Result<(tempfile::TempDir, String)> {
    let vcs = vcs.clone();
    tokio::task::spawn_blocking(move || checkout_source(config, &vcs)).await.into_diagnostic()?
}

fn cache_path(config: &pnpm_config::Config, vcs: &LockedVcs, commit: &str) -> PathBuf {
    config.cache_dir
        .join("python-git-v2")
        .join(pnpm_crypto_hash::create_hex_hash(&format!("{}@{commit}", vcs.url)))
}

fn checkout_source(
    config: &pnpm_config::Config,
    vcs: &LockedVcs,
) -> Result<(tempfile::TempDir, String)> {
    let temporary = tempfile::tempdir().into_diagnostic()?;
    let commit = checkout_repository(config, vcs, temporary.path())?;
    if !vcs.commit_id.is_empty() && commit != vcs.commit_id {
        bail!("Python git checkout does not match the locked commit");
    }
    if vcs.commit_id.is_empty() || !cache_path(config, vcs, &vcs.commit_id).is_dir() {
        pnpm_git_fetcher::checkout_submodules(temporary.path()).into_diagnostic()?;
    }
    if !config.offline {
        cache_checkout(temporary.path(), &cache_path(config, vcs, &commit))?;
    }
    Ok((temporary, commit))
}

fn checkout_repository(
    config: &pnpm_config::Config,
    vcs: &LockedVcs,
    dest: &Path,
) -> Result<String> {
    let cached = cache_path(config, vcs, &vcs.commit_id);
    let revision = if vcs.commit_id.is_empty() { &vcs.requested_revision } else { &vcs.commit_id };
    let commit = if !vcs.commit_id.is_empty() && cached.is_dir() {
        pnpm_git_fetcher::checkout_cached_bundles(&cached, revision, dest).into_diagnostic()?
    } else {
        if config.offline {
            bail!(
                "Python git repository {} is not cached for offline installation",
                pnpm_network::redact_and_sanitize(&vcs.url),
            );
        }
        pnpm_git_fetcher::checkout_revision(&vcs.url, revision, dest).into_diagnostic()?
    };
    Ok(commit)
}

fn cache_checkout(checkout: &Path, cache: &Path) -> Result<()> {
    if cache.is_dir() {
        return Ok(());
    }
    let parent = cache.parent().expect("cache has parent");
    std::fs::create_dir_all(parent).into_diagnostic()?;
    let cached = tempfile::tempdir_in(parent).into_diagnostic()?;
    pnpm_git_fetcher::cache_checkout_bundles(checkout, cached.path()).into_diagnostic()?;
    match std::fs::rename(cached.path(), cache) {
        Ok(()) => {
            let _ = cached.keep();
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::DirectoryNotEmpty,
            ) && cache.is_dir() => {}
        Err(error) => return Err(error).into_diagnostic(),
    }
    Ok(())
}
