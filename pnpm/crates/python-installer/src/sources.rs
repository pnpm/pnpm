mod git;
mod sdist;
mod wheel;

pub(super) const MAX_WHEEL_BYTES: usize = 512 * 1024 * 1024;

use super::{
    build,
    environment::PythonPrepare,
    host,
    manifest::{
        Manifest,
        Source,
    },
    registry::Registry,
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
    bail,
};
use pep440_rs::Version;
use pep508_rs::{
    PackageName,
    Requirement,
    VersionOrUrl,
};
use pnpm_python_resolver::{
    Candidate,
    LockedVcs,
};
use pnpm_reporter::Reporter;
use std::{
    collections::BTreeMap,
    path::Path,
};

pub(super) struct Sources<'a> {
    pub(super) prepare: &'a PythonPrepare<'a>,
    pub(super) built: BTreeMap<(PackageName, Version), build::Built>,
    pub(super) fetched: BTreeMap<(PackageName, Version), Candidate>,
}

impl<'a> Sources<'a> {
    pub(super) fn new(prepare: &'a PythonPrepare<'a>) -> Self {
        Self { prepare, built: BTreeMap::new(), fetched: BTreeMap::new() }
    }
}

pub(super) fn declaration_url(source: &Source) -> Result<Option<String>> {
    let count = usize::from(source.git.is_some())
        + usize::from(source.url.is_some())
        + usize::from(source.path.is_some())
        + usize::from(source.workspace);
    if count > 1 {
        bail!("a Python source must declare exactly one of git, url, path or workspace");
    }
    if (source.git.is_some() || source.url.is_some()) && source.editable == Some(true) {
        bail!("Python git and URL sources cannot be installed editable");
    }
    let Some(git) = &source.git else { return Ok(source.url.clone()) };
    let revisions = [&source.revision.rev, &source.revision.tag, &source.revision.branch];
    if revisions
        .iter()
        .filter(|revision| revision.is_some())
        .count()
        > 1
    {
        bail!("a Python git source must declare only one of rev, tag or branch");
    }
    let revision = revisions
        .into_iter()
        .flatten()
        .next()
        .map_or("HEAD", String::as_str);
    let fragment = source.revision.subdirectory
        .as_ref()
        .map_or_else(String::new, |directory| {
            format!("#subdirectory={}", pnpm_network::encode_uri_component(directory))
        });
    let revision = pnpm_network::encode_uri_component(revision);
    let mut url: url::Url = git.parse().into_diagnostic()?;
    url.set_path(&format!("{}@{revision}", url.path()));
    Ok(Some(format!("git+{url}{fragment}")))
}

impl Registry<'_> {
    pub(super) fn has_wheel(&self, name: &PackageName, version: &Version) -> bool {
        let key = (name.clone(), version.clone());
        let Some(fetched) = self.sources.fetched.get(&key) else { return false };
        let Some(wheel) = self.wheels.get(&key) else { return false };
        let Some(selected) = self.resolution.packages.candidates
            .get(name)
            .and_then(|versions| versions.get(version))
        else {
            return false;
        };
        match (fetched, selected) {
            (Candidate::Wheel(a), Candidate::Wheel(b)) => {
                wheel.filename == b.wheel.name
                    && a.wheel.name == b.wheel.name
                    && a.wheel.url == b.wheel.url
                    && a.wheel.hashes == b.wheel.hashes
            }
            (Candidate::Vcs(a), Candidate::Vcs(b)) => {
                a == b && self.check_source_wheel(wheel, name, version).is_ok()
            }
            (Candidate::Sdist(a), Candidate::Sdist(b)) => {
                a == b && self.check_source_wheel(wheel, name, version).is_ok()
            }
            _ => false,
        }
    }

    /// What the resolution asked for when it named a version: the
    /// wheel's own `METADATA`, or the metadata of the wheel the release's
    /// source distribution builds.
    pub(super) async fn fetch_metadata<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        version: &Version,
    ) -> Result<()> {
        if self.resolution.packages.candidates
            .get(name)
            .and_then(|versions| versions.get(version))
            .is_some_and(|candidate| candidate.sdist().is_some())
        {
            return self.build_sdist::<Reporter>(name, version).await;
        }
        self.fetch_wheel::<Reporter>(name, version).await
    }

    /// Build every source distribution the selected candidates hold that
    /// this run has not built already, which after a lockfile is seeded
    /// is each release it pins no wheel for.
    pub(super) async fn build_sdists<Reporter: self::Reporter + 'static>(&mut self) -> Result<()> {
        for (name, version) in self.wanted_sdists() {
            self.build_sdist::<Reporter>(&name, &version).await?;
        }
        Ok(())
    }

    fn wanted_sdists(&self) -> Vec<(PackageName, Version)> {
        let mut wanted = Vec::new();
        for (name, versions) in &self.resolution.packages.candidates {
            for (version, candidate) in versions {
                if candidate.sdist().is_some() && !self.has_wheel(name, version) {
                    wanted.push((name.clone(), version.clone()));
                }
            }
        }
        wanted
    }

    pub(super) async fn fetch_source<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        source: &str,
    ) -> Result<()> {
        self.resolution.packages.direct_urls.insert(name.clone(), source.to_string());
        let parsed = pnpm_python_resolver::Source::parse(source)?;
        if self.reuse_source(name, &parsed)? {
            return Ok(());
        }
        match parsed {
            pnpm_python_resolver::Source::Git(vcs) => self.fetch_git::<Reporter>(name, vcs).await?,
            pnpm_python_resolver::Source::Wheel { .. } => {
                self.fetch_url_wheel::<Reporter>(name, source).await?;
            }
        }
        Ok(())
    }

    fn reuse_source(
        &mut self,
        name: &PackageName,
        source: &pnpm_python_resolver::Source,
    ) -> Result<bool> {
        let Some((key, candidate)) = self.sources.fetched
            .iter()
            .find(|((distribution, _), candidate)| {
                distribution == name && candidate.matches_source(source)
            })
            .map(|(key, candidate)| (key.clone(), candidate.clone()))
        else {
            return Ok(false);
        };
        let Some(wheel) = self.wheels.get(&key).cloned() else { return Ok(false) };
        self.check_source_wheel(&wheel, name, &key.1)?;
        if let Some(artifact) = candidate.wheel() {
            artifact.check_installable(&self.resolution.target.tags, name, &key.1)?;
        }
        self.resolution.packages.candidates.insert(
            name.clone(),
            BTreeMap::from([(key.1.clone(), candidate)]),
        );
        self.remember(name.clone(), key.1, wheel);
        Ok(true)
    }

    pub(super) fn check_source_wheel(
        &self,
        wheel: &host::Wheel,
        name: &PackageName,
        version: &Version,
    ) -> Result<()> {
        let Some((wheel_name, wheel_version, _)) =
            pnpm_python_resolver::wheel_identity(&wheel.filename, &self.resolution.target.tags)?
        else {
            bail!("Python wheel is incompatible with this interpreter: {}", wheel.filename);
        };
        if wheel_name != *name || wheel_version != *version {
            bail!("Python source wheel identity mismatch: {}", wheel.filename);
        }
        Ok(())
    }

    pub(super) async fn fetch_vcs<Reporter: self::Reporter + 'static>(
        &mut self,
        requirements: &[Requirement],
    ) -> Result<()> {
        let mut wanted = self.wanted_vcs();
        for (name, version, _) in &wanted {
            self.resolution.packages.metadata.remove(&(name.clone(), version.clone()));
        }
        while !wanted.is_empty() {
            let active = pnpm_python_resolver::active_locked_sources(
                &self.resolution.packages,
                requirements,
                &self.resolution.target.environment,
            )?;
            let Some(position) = wanted
                .iter()
                .position(|(name, _, _)| active.contains(name))
            else {
                bail!("Python lockfile selected an inactive source");
            };
            let (name, version, vcs) = wanted.remove(position);
            self.fetch_git::<Reporter>(&name, vcs).await?;
            if !self.has_wheel(&name, &version) {
                bail!("the Python git source built a different version of {name}=={version}");
            }
            self.record_sources(&[])?;
        }
        Ok(())
    }

    fn wanted_vcs(&self) -> Vec<(PackageName, Version, LockedVcs)> {
        let mut wanted = Vec::new();
        for (name, versions) in &self.resolution.packages.candidates {
            for (version, candidate) in versions {
                if let Candidate::Vcs(vcs) = candidate
                    && !self.has_wheel(name, version)
                {
                    wanted.push((name.clone(), version.clone(), vcs.clone()));
                }
            }
        }
        wanted
    }

    pub(super) fn record_sources(&mut self, requirements: &[Requirement]) -> Result<()> {
        let mut declared = requirements.to_vec();
        for ((name, version), metadata) in &self.resolution.packages.metadata {
            if !self.resolution.packages.candidates
                .get(name)
                .is_some_and(|versions| versions.contains_key(version))
            {
                continue;
            }
            for requirement in &metadata.requires_dist {
                declared.push(requirement.parse::<Requirement>().into_diagnostic()?);
            }
        }
        for requirement in declared {
            let Some(VersionOrUrl::Url(url)) = &requirement.version_or_url else { continue };
            let source = pnpm_python_resolver::Source::parse(url.as_str())?;
            if self.resolution.packages.candidates
                .get(&requirement.name)
                .is_some_and(|versions| {
                    versions
                        .values()
                        .all(|candidate| candidate.matches_source(&source))
                })
            {
                self.resolution.packages.direct_urls
                    .entry(requirement.name)
                    .or_insert_with(|| url.to_string());
            }
        }
        Ok(())
    }

    pub(super) fn source_provenance(&self, name: &PackageName) -> Option<host::DirectUrl> {
        let source = self.resolution.packages.direct_urls.get(name)?;
        if source.starts_with("git+") {
            return None;
        }
        let candidate = self.resolution.packages.candidates
            .get(name)?
            .values()
            .next()?
            .wheel()?;
        Some(host::DirectUrl::archive(candidate))
    }
}

/// The manifest of a source pnpm downloaded, which a project built by a
/// `setup.py` alone does not have: PEP 517 has such a project built by
/// setuptools' legacy backend, as if it declared nothing.
pub(super) async fn read_manifest(root: &Path) -> Result<Manifest> {
    match tokio::fs::read_to_string(root.join("pyproject.toml")).await {
        Ok(contents) => Manifest::parse(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Manifest::parse(""),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}/pyproject.toml", root.display())),
    }
}

#[cfg(test)]
mod tests;
