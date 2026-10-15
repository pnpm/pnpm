use super::super::registry::{CachedIndex, Fetcher, Registry, checked_metadata_digests};
use futures_util::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use miette::Result;
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{ExtraName, PackageName, Requirement, VersionOrUrl};
use pnpm_python_resolver::{IndexCandidate, WheelMetadata, parse_requirement};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
};

/// Index reads started before the resolution asks for them.
///
/// The solver names one missing piece at a time. Waiting for each in turn
/// costs a round trip per index page and per metadata file, so every page
/// and metadata file a resolution will likely ask for is read as soon as
/// what names it is known: the pages of what a requirement or a read
/// metadata file names, and the metadata of the version the solver will
/// likely pick from a page. What the solver asks for is still checked
/// the same way. A wrong guess costs a read and nothing else.
pub(super) struct Ahead<'a> {
    fetcher: Fetcher<'a>,
    reads: FuturesUnordered<BoxFuture<'a, Read>>,
    /// Distributions whose index page is being read or was recorded.
    pages: BTreeSet<PackageName>,
    /// Versions whose metadata file is being read or was recorded.
    metadata: BTreeSet<(PackageName, Version)>,
    /// Versions whose index publishes no metadata file to read, so the
    /// resolution reads the wheel instead.
    unpublished: BTreeSet<(PackageName, Version)>,
    /// What the requirements read so far ask of each distribution, which
    /// picks the version whose metadata is read ahead.
    wanted: BTreeMap<PackageName, Wanted>,
    /// Distributions a requirement read so far takes from a direct URL,
    /// which the index must not be asked about.
    direct: BTreeSet<PackageName>,
    /// Reads that finished while the registry was busy with something else.
    finished: Vec<Read>,
}

#[derive(Default)]
struct Wanted {
    specifiers: Vec<VersionSpecifiers>,
    extras: BTreeSet<ExtraName>,
}

enum Read {
    Page(PackageName, Result<CachedIndex>),
    Metadata(PackageName, Version, Result<Option<WheelMetadata>>),
}

impl Read {
    fn is_page(&self, of: &PackageName) -> bool {
        matches!(self, Self::Page(name, _) if name == of)
    }

    fn is_metadata(&self, (of, at): &(PackageName, Version)) -> bool {
        matches!(self, Self::Metadata(name, version, _) if name == of && version == at)
    }
}

impl<'a> Ahead<'a> {
    pub(super) fn new(registry: &Registry<'a>) -> Self {
        Self {
            fetcher: registry.fetcher(),
            reads: FuturesUnordered::new(),
            pages: BTreeSet::new(),
            metadata: BTreeSet::new(),
            unpublished: BTreeSet::new(),
            wanted: BTreeMap::new(),
            direct: BTreeSet::new(),
            finished: Vec::new(),
        }
    }

    pub(super) fn read_requirements(
        &mut self,
        registry: &Registry<'a>,
        requirements: &[Requirement],
        extras: &[ExtraName],
    ) {
        // A requirement can name a distribution both plainly and with a
        // direct URL, which the solver then takes it from.
        self.direct.extend(
            requirements
                .iter()
                .filter(|requirement| {
                    matches!(requirement.version_or_url, Some(VersionOrUrl::Url(_)))
                })
                .map(|requirement| requirement.name.clone()),
        );
        for requirement in requirements {
            self.want(registry, requirement, extras);
        }
    }

    /// Forget what was recorded, after the resolution went back to an
    /// earlier state that lacks it. Reads still in flight are recorded
    /// into that state when they finish.
    pub(super) fn backtracked(&mut self) {
        self.pages.clear();
        self.metadata.clear();
    }

    /// Make sure the candidates of `name` are recorded.
    pub(super) async fn candidates(
        &mut self,
        registry: &mut Registry<'a>,
        name: &PackageName,
    ) -> Result<()> {
        self.record_finished(registry);
        self.read_page(registry, name);
        let recorded = |_: &Self, registry: &Registry<'a>| {
            registry.resolution.packages.candidates.contains_key(name)
        };
        if !self.record_until(registry, recorded, |read| read.is_page(name)).await? {
            miette::bail!("the Python index page of {name} was never read");
        }
        Ok(())
    }

    /// Make sure the metadata of `name` `version` is recorded, from the
    /// metadata file its index publishes or else out of the wheel.
    pub(super) async fn metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &mut self,
        registry: &mut Registry<'a>,
        name: &PackageName,
        version: &Version,
    ) -> Result<()> {
        self.record_finished(registry);
        let key = (name.clone(), version.clone());
        if let Some(candidate) = registry.resolution.packages.candidates
            .get(name)
            .and_then(|versions| versions.get(version))
            .and_then(|candidate| candidate.from_index())
            .cloned()
        {
            self.read_metadata(registry, name, version, &candidate);
        }
        let settled = |ahead: &Self, registry: &Registry<'a>| {
            !ahead.metadata.contains(&key)
                || registry.resolution.packages.metadata.contains_key(&key)
        };
        self.record_until(registry, settled, |read| read.is_metadata(&key)).await?;
        if registry.resolution.packages.metadata.contains_key(&key) {
            return Ok(());
        }
        self.while_reading(registry.fetch_metadata::<Reporter>(name, version)).await
    }

    /// Record reads as they finish until `done`, or `false` once none is
    /// left. Fails when the read `wanted` names fails.
    async fn record_until(
        &mut self,
        registry: &mut Registry<'a>,
        done: impl Fn(&Self, &Registry<'a>) -> bool,
        wanted: impl Fn(&Read) -> bool,
    ) -> Result<bool> {
        while !done(self, registry) {
            let Some(read) = self.reads.next().await else { return Ok(false) };
            let is_wanted = wanted(&read);
            if let Err(error) = self.record(registry, read) {
                if is_wanted {
                    return Err(error);
                }
                forget(&error);
            }
        }
        Ok(true)
    }

    /// Run `work` on the registry, reading ahead meanwhile. What finishes
    /// is recorded once the registry is free again.
    pub(super) async fn while_reading<Output>(
        &mut self,
        work: impl Future<Output = Output>,
    ) -> Output {
        let mut work = Box::pin(work);
        loop {
            tokio::select! {
                biased;
                output = &mut work => return output,
                Some(read) = self.reads.next(), if !self.reads.is_empty() => self.finished.push(read),
            }
        }
    }

    fn record_finished(&mut self, registry: &mut Registry<'a>) {
        while let Some(Some(read)) = self.reads.next().now_or_never() {
            self.finished.push(read);
        }
        for read in std::mem::take(&mut self.finished) {
            if let Err(error) = self.record(registry, read) {
                forget(&error);
            }
        }
    }

    /// Record a finished read and start the reads it makes likely. A
    /// failed read is forgotten, so asking for it again reads it again.
    fn record(&mut self, registry: &mut Registry<'a>, read: Read) -> Result<()> {
        match read {
            Read::Page(name, page) => self.record_page(registry, &name, page),
            Read::Metadata(name, version, metadata) => {
                self.record_metadata(registry, (name, version), metadata)
            }
        }
    }

    fn record_page(
        &mut self,
        registry: &mut Registry<'a>,
        name: &PackageName,
        page: Result<CachedIndex>,
    ) -> Result<()> {
        let recorded = page.and_then(|page| {
            // Something else offered this distribution meanwhile, such as
            // a direct URL requirement.
            if registry.resolution.packages.candidates.contains_key(name) {
                return Ok(());
            }
            registry.record_index(name, &page)
        });
        if let Err(error) = recorded {
            self.pages.remove(name);
            return Err(error);
        }
        self.read_likely_metadata(registry, name);
        Ok(())
    }

    fn record_metadata(
        &mut self,
        registry: &mut Registry<'a>,
        key: (PackageName, Version),
        metadata: Result<Option<WheelMetadata>>,
    ) -> Result<()> {
        let metadata = match metadata {
            Ok(Some(metadata)) => metadata,
            Ok(None) => {
                self.metadata.remove(&key);
                self.unpublished.insert(key);
                return Ok(());
            }
            Err(error) => {
                self.metadata.remove(&key);
                return Err(error);
            }
        };
        let extras = self.wanted
            .get(&key.0)
            .map(|wanted| {
                wanted.extras
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let requirements = metadata.requires_dist
            .iter()
            .filter_map(|declared| parse_requirement(declared).ok())
            .collect::<Vec<_>>();
        self.read_requirements(registry, &requirements, &extras);
        registry.resolution.packages.metadata.entry(key).or_insert(metadata);
        Ok(())
    }

    /// Note what `requirement` asks for, and read what it names.
    fn want(&mut self, registry: &Registry<'a>, requirement: &Requirement, extras: &[ExtraName]) {
        if !requirement.marker.evaluate(&registry.resolution.target.environment, extras)
            || self.direct.contains(&requirement.name)
            || registry.resolution.packages.direct_urls.contains_key(&requirement.name)
        {
            return;
        }
        let wanted = self.wanted.entry(requirement.name.clone()).or_default();
        match &requirement.version_or_url {
            Some(VersionOrUrl::Url(_)) => return,
            Some(VersionOrUrl::VersionSpecifier(specifiers)) => {
                wanted.specifiers.push(specifiers.clone());
            }
            None => {}
        }
        wanted.extras.extend(requirement.extras.iter().cloned());
        if registry.resolution.packages.candidates.contains_key(&requirement.name) {
            self.read_likely_metadata(registry, &requirement.name);
        } else {
            self.read_page(registry, &requirement.name);
        }
    }

    fn read_page(&mut self, registry: &Registry<'a>, name: &PackageName) {
        if registry.resolution.packages.candidates.contains_key(name)
            || !self.pages.insert(name.clone())
        {
            return;
        }
        let replayed = registry.replays_index(name);
        let fetcher = self.fetcher;
        let name = name.clone();
        self.reads.push(
            async move {
                let page = fetcher.index_page(&name, replayed).await;
                Read::Page(name, page)
            }
            .boxed(),
        );
    }

    /// Read the metadata of the version of `name` the solver will likely
    /// pick: the highest one every requirement read so far admits, and a
    /// prerelease only when nothing else is.
    fn read_likely_metadata(&mut self, registry: &Registry<'a>, name: &PackageName) {
        let Some(versions) = registry.resolution.packages.candidates.get(name) else { return };
        let specifiers = self.wanted
            .get(name)
            .map(|wanted| &wanted.specifiers[..])
            .unwrap_or(&[][..]);
        let admitted = |version: &&Version| {
            specifiers
                .iter()
                .all(|specifiers| specifiers.contains(version))
        };
        let likely = versions
            .keys()
            .rev()
            .filter(admitted)
            .find(|version| !version.any_prerelease())
            .or_else(|| versions.keys().rev().find(admitted));
        let Some(version) = likely.cloned() else { return };
        if let Some(candidate) = versions[&version].from_index().cloned() {
            self.read_metadata(registry, name, &version, &candidate);
        }
    }

    fn read_metadata(
        &mut self,
        registry: &Registry<'a>,
        name: &PackageName,
        version: &Version,
        candidate: &IndexCandidate,
    ) {
        let key = (name.clone(), version.clone());
        if checked_metadata_digests(candidate).is_none()
            || registry.resolution.packages.metadata.contains_key(&key)
            || self.unpublished.contains(&key)
            // A wheel this target cannot install is refused where the
            // resolution reads it, not here.
            || registry.check_index_wheel(&candidate.wheel, name, version).is_err()
            || !self.metadata.insert(key)
        {
            return;
        }
        let fetcher = self.fetcher;
        let (name, version, candidate) = (name.clone(), version.clone(), candidate.clone());
        self.reads.push(
            async move {
                let metadata = fetcher.core_metadata(&name, &version, &candidate).await;
                Read::Metadata(name, version, metadata)
            }
            .boxed(),
        );
    }
}

/// A read ahead that failed is left to fail where the resolution asks for
/// it, which reads it again.
fn forget(error: &miette::Report) {
    tracing::debug!(target: "pnpm::python", "a Python index read ahead failed: {error:?}");
}
