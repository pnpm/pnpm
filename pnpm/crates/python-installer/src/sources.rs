mod git;
mod wheel;

use super::{build, environment::PythonPrepare, host, manifest::Source, registry::Registry};
use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::Version;
use pep508_rs::{PackageName, Requirement, VersionOrUrl};
use pnpm_python_resolver::Candidate;
use pnpm_reporter::Reporter;
use std::collections::BTreeMap;

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
    Ok(Some(format!("git+{git}@{revision}{fragment}")))
}

impl Registry<'_> {
    pub(super) fn has_wheel(&self, name: &PackageName, version: &Version) -> bool {
        let key = (name.clone(), version.clone());
        let Some(fetched) = self.sources.fetched.get(&key) else { return false };
        let Some(wheel) = self.wheels.get(&key) else { return false };
        let selected = &self.resolution.packages.candidates[name][version];
        match (fetched, selected) {
            (Candidate::Wheel(a), Candidate::Wheel(b)) => {
                wheel.filename == b.wheel.name
                    && a.wheel.name == b.wheel.name
                    && a.wheel.url == b.wheel.url
                    && a.wheel.hashes == b.wheel.hashes
            }
            (Candidate::Vcs(a), Candidate::Vcs(b)) => a == b,
            _ => false,
        }
    }

    pub(super) async fn fetch_source<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        source: &str,
    ) -> Result<()> {
        self.resolution.packages.direct_urls.insert(name.clone(), source.to_string());
        match pnpm_python_resolver::Source::parse(source)? {
            pnpm_python_resolver::Source::Git(vcs) => self.fetch_git::<Reporter>(name, vcs).await?,
            pnpm_python_resolver::Source::Wheel { .. } => {
                self.fetch_url_wheel::<Reporter>(name, source).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn fetch_vcs<Reporter: self::Reporter + 'static>(&mut self) -> Result<()> {
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
        for (name, version, vcs) in wanted {
            self.fetch_git::<Reporter>(&name, vcs).await?;
            if !self.has_wheel(&name, &version) {
                bail!("the Python git source built a different version of {name}=={version}");
            }
        }
        Ok(())
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
