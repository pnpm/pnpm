//! The lockfile a project installs from: the one on disk when it still
//! applies to the project on this target, else the one the pnpr server
//! resolves, else a local resolution.

use super::{
    Inputs,
    Lockfile,
    Registry,
    environment::{
        LockfileInputs,
        LockfileReplay,
        PythonPrepare,
        accept_server_lockfile,
        read_existing_lock,
        resolve_via_pnpr,
    },
    projects,
    resolver,
    workspace,
};
use miette::Result;
use pnpm_reporter::{
    GlobalLog,
    LogEvent,
    LogLevel,
    Reporter,
};
use std::{
    path::Path,
    sync::Arc,
};

impl PythonPrepare<'_> {
    /// The lockfile on disk when this install may replay it: one that
    /// applies to the project on this target, for an install that is not
    /// adding a dependency. A frozen install fails on anything else.
    pub(super) async fn replayable_lockfile(
        &self,
        lock_path: &Path,
        inputs: &Inputs,
        requires_python: Option<&str>,
        local: &[workspace::LocalProject],
    ) -> Result<Option<Lockfile>> {
        let existing = read_existing_lock(lock_path).await?;
        let stale = if self.asked.resolve {
            Some(miette::miette!("adding a dependency resolves the project again"))
        } else {
            match &existing {
                Some(lock) => lock
                    .applies_to(inputs, requires_python, &self.interpreter.target)
                    .and_then(|()| workspace::describes(lock, local))
                    .err(),
                None => Some(miette::miette!("the project has no lockfile")),
            }
        };
        match stale {
            None => Ok(existing),
            Some(reason) if self.context.frozen_lockfile => Err(reason.wrap_err(format!(
                "frozen Python lockfile is missing or out of date: {}",
                lock_path.display(),
            ))),
            Some(_) => Ok(None),
        }
    }

    /// The lockfile for one project: the one on disk when it still covers
    /// the project on this target, then the one the server resolves, and a
    /// local resolution last.
    pub(super) async fn lockfile<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        inputs: LockfileInputs<'_>,
    ) -> Result<Lockfile> {
        let LockfileInputs {
            existing,
            lock_path,
            requirements,
            inputs,
            requires_python,
            local,
            members,
        } = inputs;
        if let Some(lock) = existing {
            let replay = LockfileReplay {
                same_target: lock.tool.pnpm == inputs,
                lock,
                lock_path,
                requirements,
                local: Arc::clone(&local),
            };
            if let Some(lock) = self.replay_lockfile::<Reporter>(registry, replay).await? {
                return Ok(lock);
            }
        }
        if registry.resolution.packages.overrides.is_empty()
            && registry.resolution.packages.constraints.is_empty()
            && let Some(lock) =
                self.resolve_remotely(requirements, (&inputs, requires_python.as_deref()), &local)
                    .await?
        {
            return self.accept_lockfile::<Reporter>(registry, lock, requirements, &local).await;
        }
        let solved =
            resolver::resolve_all::<Reporter>(registry, requirements, &self.environments.list)
                .await
                .map_err(|error| projects::disagreement(&registry.resolution, members, error))?;
        Lockfile::merged(
            &registry.resolution.packages.metadata,
            requirements,
            &solved,
            inputs,
            requires_python,
        )
    }

    /// The lockfile a pnpr server resolves, when one can answer this
    /// project, checked to answer `inputs`. A server resolves one
    /// interpreter's environment from an index, so a project that declares
    /// the environments it locks for is resolved here instead, as is one
    /// that installs a project from this repository.
    async fn resolve_remotely(
        &self,
        requirements: &[pep508_rs::Requirement],
        (inputs, requires_python): (&Inputs, Option<&str>),
        local: &[workspace::LocalProject],
    ) -> Result<Option<Lockfile>> {
        if self.environments.declared
            || !local.is_empty()
            || requirements
                .iter()
                .any(|requirement| {
                    matches!(requirement.version_or_url, Some(pep508_rs::VersionOrUrl::Url(_)))
                })
            || !self.index.can_resolve_remotely()
        {
            return Ok(None);
        }
        let Some(mut lock) = resolve_via_pnpr(
            self.context.config,
            requirements,
            &self.interpreter.target,
            self.index.url.as_str(),
            requires_python.map(str::to_string),
        )
        .await?
        else {
            return Ok(None);
        };
        // A server answers for the requirements, not for which projects
        // asked them together: the members are this install's to record.
        lock.tool.pnpm.set_members(inputs.members().to_vec());
        accept_server_lockfile(&lock, inputs, requires_python)?;
        Ok(Some(lock))
    }

    /// Replay the lockfile on disk, or `None` when an install that may
    /// resolve again should: its wheels install here but the interpreter's
    /// markers no longer select its graph, or it was resolved for another
    /// target and pins a wheel this install cannot fetch. A frozen install
    /// fails on either, and every install fails on a wheel a lockfile
    /// resolved for this very target cannot fetch: that lockfile pins only
    /// wheels the target needs.
    async fn replay_lockfile<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        LockfileReplay {
            lock,
            lock_path,
            requirements,
            local,
            same_target,
        }: LockfileReplay<'_>,
    ) -> Result<Option<Lockfile>> {
        registry.resolution.packages.candidates.clear();
        lock.seed(&mut registry.resolution.packages, &self.interpreter.target)?;
        workspace::offer_locked(&mut registry.resolution.packages, &local, &lock);
        registry.record_sources(requirements)?;
        let replayed = match registry.fetch_wheels::<Reporter>(requirements).await {
            Ok(()) => resolver::validate_locked(&registry.resolution, requirements),
            Err(error) if same_target => return Err(error),
            Err(error) => Err(error),
        };
        let Err(error) = replayed else {
            return Ok(Some(lock));
        };
        if self.context.frozen_lockfile {
            return Err(error);
        }
        Reporter::emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Warn,
            message: format!("Ignoring Python lockfile {}: {error}", lock_path.display()),
        }));
        registry.resolution.packages.candidates.clear();
        registry.resolution.packages.excluded.clear();
        registry.resolution.packages.metadata.clear();
        registry.resolution.packages.direct_urls.clear();
        registry.resolution.packages.rejected_sources.clear();
        workspace::offer(&mut registry.resolution.packages, &local);
        Ok(None)
    }

    /// Fetch the wheels a ready-made lockfile pins and check that it still
    /// covers the project's requirements.
    pub(super) async fn accept_lockfile<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        lock: Lockfile,
        requirements: &[pep508_rs::Requirement],
        local: &[workspace::LocalProject],
    ) -> Result<Lockfile> {
        registry.resolution.packages.candidates.clear();
        lock.seed(&mut registry.resolution.packages, &self.interpreter.target)?;
        workspace::offer_locked(&mut registry.resolution.packages, local, &lock);
        registry.record_sources(requirements)?;
        registry.fetch_wheels::<Reporter>(requirements).await?;
        resolver::validate_locked(&registry.resolution, requirements)?;
        Ok(lock)
    }
}
