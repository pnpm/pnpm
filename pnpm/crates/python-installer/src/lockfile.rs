//! The lockfile a project installs from: the one on disk when it still
//! applies to the project on this target, else the one the pnpr server
//! resolves, else a local resolution.

use super::{
    Inputs, Lockfile, Registry,
    environment::{
        LockfileInputs, LockfileReplay, PythonPrepare, accept_server_lockfile, read_existing_lock,
        resolve_via_pnpr,
    },
    resolver,
};
use miette::Result;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use std::path::Path;

impl PythonPrepare<'_> {
    /// The lockfile on disk when this install may replay it: one that
    /// applies to the project on this target, for an install that is not
    /// adding a dependency. A frozen install fails on anything else.
    pub(super) async fn replayable_lockfile(
        &self,
        lock_path: &Path,
        inputs: &Inputs,
        requires_python: Option<&str>,
    ) -> Result<Option<Lockfile>> {
        let existing = read_existing_lock(lock_path).await?;
        let stale = if self.resolve {
            Some(miette::miette!("adding a dependency resolves the project again"))
        } else {
            match &existing {
                Some(lock) => {
                    lock.applies_to(inputs, requires_python, &self.interpreter.target).err()
                }
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
        } = inputs;
        if let Some(lock) = existing {
            let replay = LockfileReplay {
                same_target: lock.tool.pnpm == inputs,
                lock,
                lock_path,
                requirements,
            };
            if let Some(lock) = self.replay_lockfile::<Reporter>(registry, replay).await? {
                return Ok(lock);
            }
        }
        if let Some(lock) = resolve_via_pnpr(
            self.context.config,
            requirements,
            &self.interpreter.target,
            self.index.as_str(),
            requires_python.clone(),
        )
        .await?
        {
            accept_server_lockfile(&lock, &inputs, requires_python.as_deref())?;
            self.accept_lockfile::<Reporter>(registry, lock, requirements).await
        } else {
            let solution = resolver::resolve::<Reporter>(registry, requirements).await?;
            Lockfile::new(
                &registry.packages,
                &self.interpreter.target,
                requirements,
                solution,
                inputs,
                requires_python,
            )
        }
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
            same_target,
        }: LockfileReplay<'_>,
    ) -> Result<Option<Lockfile>> {
        lock.seed(&mut registry.packages)?;
        let replayed = match registry.fetch_wheels::<Reporter>(&lock.packages).await {
            Ok(()) => resolver::validate_locked(registry, requirements),
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
        registry.packages = pnpm_python_resolver::Packages::new();
        Ok(None)
    }

    /// Fetch the wheels a ready-made lockfile pins and check that it still
    /// covers the project's requirements.
    async fn accept_lockfile<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        lock: Lockfile,
        requirements: &[pep508_rs::Requirement],
    ) -> Result<Lockfile> {
        lock.seed(&mut registry.packages)?;
        registry.fetch_wheels::<Reporter>(&lock.packages).await?;
        resolver::validate_locked(registry, requirements)?;
        Ok(lock)
    }
}
