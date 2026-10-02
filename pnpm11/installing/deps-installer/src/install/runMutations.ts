import { LOCKFILE_MAJOR_VERSION } from '@pnpm/constants'
import { PRE_UNINSTALL_STAGES, runLifecycleHooksConcurrently } from '@pnpm/exec.lifecycle'
import type { PnpmContext } from '@pnpm/installing.context'
import type { ProjectRootDir } from '@pnpm/types'

import { collectProjectsToInstall } from './collectProjectsToInstall.js'
import { tryFastLockfileUpdate } from './fastLockfileUpdate.js'
import { InconsistentPatchHashError, UncheckablePatchHashError } from './frozenInstallErrors.js'
import { getStaleOverrideTargets } from './getStaleOverrideTargets.js'
import { installInContext } from './installInContext.js'
import { isCheckOnlyInstall, removesAnyDependency, setUntrackedPnpmfileReadPackageHook } from './installPredicates.js'
import {
  detectOutdatedLockfileSettings,
  type InstallChecksums,
  type LockfileSettingsState,
  readInstallChecksums,
  stageAddedManifests,
} from './lockfileState.js'
import type { ImporterToUpdate, InnerInstallResult, MutatedProject, MutationRun } from './mutationTypes.js'
import { removeDepsFromManifests } from './removeDepsFromManifests.js'
import { tryFrozenInstall } from './tryFrozenInstall.js'

interface LockfileUpdatePlan {
  addedManifestsAreCommitted: boolean
  checksums: InstallChecksums
  didFastUpdateOverrides: boolean
  needsFullResolution: boolean
  projectDirsRemovingDeps: Set<ProjectRootDir>
  staleOverrideTargets: Set<string> | undefined
  upToDateLockfileMajorVersion: boolean
}

export async function runMutations (run: MutationRun): Promise<InnerInstallResult> {
  const plan = await planLockfileUpdate(run)
  const frozenInstallResult = await tryFrozenInstall(run, {
    addedManifestsAreCommitted: plan.addedManifestsAreCommitted,
    didFastUpdateOverrides: plan.didFastUpdateOverrides,
    frozenLockfile: plan.checksums.frozenLockfile,
    needsFullResolution: plan.needsFullResolution,
    patchGroups: plan.checksums.patchGroups,
    projectDirsRemovingDeps: plan.projectDirsRemovingDeps,
    untrackedReadPackageHookMayHaveChanged: plan.checksums.untrackedReadPackageHookMayHaveChanged,
    upToDateLockfileMajorVersion: plan.upToDateLockfileMajorVersion,
  })
  let { needsFullResolution } = plan
  if (frozenInstallResult !== null) {
    if (!('needsFullResolution' in frozenInstallResult)) return frozenInstallResult
    needsFullResolution = frozenInstallResult.needsFullResolution
  }
  if (plan.checksums.pnpmfileChecksumIgnored) {
    needsFullResolution = true
    run.ctx.wantedLockfile.pnpmfileChecksum = plan.checksums.pnpmfileChecksum
  }

  const projectsToInstall = await collectProjectsToInstall(run)
  return installCollectedProjects(run, { needsFullResolution, plan, projectsToInstall })
}

async function planLockfileUpdate (run: MutationRun): Promise<LockfileUpdatePlan> {
  // Read before `removeDeps` edits the manifests below.
  const projectDirsRemovingDeps = findProjectDirsRemovingDeps(run)
  await runPreUninstallHooks(run, projectDirsRemovingDeps)
  const checksums = await readInstallChecksums(run)
  // `uninstallSome` edits the manifests here, ahead of the fast-path
  // dispatch, so a removal is visible to it the same way a hand-edited
  // manifest is. The resolution path is unaffected: it drops the same
  // names through `removePackages`, and `removeDeps` re-applied there
  // is a no-op.
  await Promise.all(run.projects.map(async (project) => removeDepsFromContextProject(run.ctx, project)))
  const additions = stageAddedManifests(run)
  const settings = detectOutdatedLockfileSettings(run, checksums)
  const upToDateLockfileMajorVersion = run.ctx.wantedLockfile.lockfileVersion.toString().startsWith(`${LOCKFILE_MAJOR_VERSION}.`)
  const fastUpdate = await tryFastLockfileUpdate(run, { additions, checksums, settings })
  const outdatedLockfileSettings = fastUpdate == null && settings.changedLockfileSettings.length > 0
  throwOnInconsistentFrozenPatchHashes(run.ctx, checksums.frozenLockfile)
  const needsFullResolution = requiresFullResolution(run, { outdatedLockfileSettings, upToDateLockfileMajorVersion })
  const staleOverrideTargets = updateWantedLockfileSettings(run, { checksums, needsFullResolution, settings })
  return {
    addedManifestsAreCommitted: additions.addedManifestsAreCommitted || fastUpdate != null,
    checksums,
    didFastUpdateOverrides: fastUpdate?.didFastUpdateOverrides ?? false,
    needsFullResolution,
    projectDirsRemovingDeps,
    staleOverrideTargets,
    upToDateLockfileMajorVersion,
  }
}

function findProjectDirsRemovingDeps ({ ctx, projects }: MutationRun): Set<ProjectRootDir> {
  return new Set(projects
    .filter((project) => project.mutation === 'uninstallSome' && removesAnyDependency(project, ctx.projects[project.rootDir]?.manifest))
    .map((project) => project.rootDir))
}

async function runPreUninstallHooks (
  { ctx, opts, scriptsOpts, verifyLockfile }: MutationRun,
  projectDirsRemovingDeps: Set<ProjectRootDir>
): Promise<void> {
  const skipsScripts = opts.ignoreScripts || opts.ignorePackageManifest || opts.lockfileOnly || isCheckOnlyInstall(opts)
  if (skipsScripts || opts.virtualStoreOnly || projectDirsRemovingDeps.size === 0) return
  await verifyLockfile?.()
  await runLifecycleHooksConcurrently({
    childConcurrency: opts.childConcurrency,
    importers: [...projectDirsRemovingDeps].map((rootDir) => ctx.projects[rootDir]),
    opts: scriptsOpts,
    projectDependencies: opts.projectDependencies,
    skipBinLinking: true,
    stages: PRE_UNINSTALL_STAGES,
  })
}

async function removeDepsFromContextProject (ctx: PnpmContext, project: MutatedProject): Promise<void> {
  if (project.mutation !== 'uninstallSome') return
  const ctxProject = ctx.projects[project.rootDir]
  if (ctxProject == null) return
  await removeDepsFromManifests(ctxProject, project)
}

/**
 * A frozen install cannot re-resolve to settle either case, and each names only what it
 * established: a suffix shown to disagree, or one that could not be judged at all. A lockfile
 * whose conflicts were autofixed falls through to resolution instead.
 */
function throwOnInconsistentFrozenPatchHashes (ctx: PnpmContext, frozenLockfile: boolean): void {
  if (!frozenLockfile || ctx.lockfileHadConflicts) return
  if (ctx.patchedDepPathsStatus === 'stale') throw new InconsistentPatchHashError()
  if (ctx.patchedDepPathsStatus === 'indeterminate') throw new UncheckablePatchHashError()
}

function requiresFullResolution (
  { ctx, forceResolutionFromHook, opts }: MutationRun,
  { outdatedLockfileSettings, upToDateLockfileMajorVersion }: { outdatedLockfileSettings: boolean, upToDateLockfileMajorVersion: boolean }
): boolean {
  return outdatedLockfileSettings ||
    ctx.patchedDepPathsStatus !== 'up-to-date' ||
    opts.fixLockfile ||
    opts.updateChecksums ||
    !upToDateLockfileMajorVersion ||
    opts.forceFullResolution ||
    forceResolutionFromHook
}

/**
 * Records the current settings in the wanted lockfile. A full resolution
 * records every setting it resolves under, and returns the packages whose
 * override changed.
 */
function updateWantedLockfileSettings (
  { ctx, opts }: MutationRun,
  { checksums, needsFullResolution, settings }: { checksums: InstallChecksums, needsFullResolution: boolean, settings: LockfileSettingsState }
): Set<string> | undefined {
  if (!needsFullResolution) {
    if (!checksums.frozenLockfile) {
      ctx.wantedLockfile.settings = { ...settings.wantedLockfileSettings }
    }
    return undefined
  }
  const staleOverrideTargets = getStaleOverrideTargets(ctx.wantedLockfile.overrides, settings.overridesMap)
  ctx.wantedLockfile.settings = { ...settings.wantedLockfileSettings }
  ctx.wantedLockfile.overrides = settings.overridesMap
  ctx.wantedLockfile.packageExtensionsChecksum = checksums.packageExtensionsChecksum
  ctx.wantedLockfile.ignoredOptionalDependencies = opts.ignoredOptionalDependencies
  ctx.wantedLockfile.pnpmfileChecksum = checksums.pnpmfileChecksum
  setUntrackedPnpmfileReadPackageHook(ctx.wantedLockfile, checksums.untrackedPnpmfileReadPackageHook)
  ctx.wantedLockfile.patchedDependencies = checksums.patchedDependencies
  return staleOverrideTargets
}

async function installCollectedProjects (
  run: MutationRun,
  { needsFullResolution, plan, projectsToInstall }: {
    needsFullResolution: boolean
    plan: LockfileUpdatePlan
    projectsToInstall: ImporterToUpdate[]
  }
): Promise<InnerInstallResult> {
  const { ctx, opts } = run
  // Unfortunately, the private lockfile may differ from the public one.
  // A user might run named installations on a project that has a pnpm-lock.yaml file before running a noop install
  const makePartialCurrentLockfile = !run.installsOnly && (
    ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile ||
    !ctx.currentLockfileIsUpToDate
  )
  const result = await installInContext(projectsToInstall, ctx, {
    ...opts,
    allowBuild: run.allowBuild,
    projectDirsRemovingDeps: plan.projectDirsRemovingDeps,
    currentLockfileIsUpToDate: !ctx.existsNonEmptyWantedLockfile || ctx.currentLockfileIsUpToDate,
    makePartialCurrentLockfile,
    needsFullResolution,
    pruneVirtualStore: run.pruneVirtualStore,
    rootProjectPreinstallRan: run.rootProjectPreinstallRan,
    scriptsOpts: run.scriptsOpts,
    staleOverrideTargets: plan.staleOverrideTargets,
    updateLockfileMinorVersion: true,
    patchedDependencies: plan.checksums.patchGroups,
    verifyLockfile: run.verifyLockfile,
  })

  return {
    updatedCatalogs: result.updatedCatalogs,
    updatedProjects: result.projects,
    newLockfile: result.newLockfile,
    stats: result.stats,
    depsRequiringBuild: result.depsRequiringBuild,
    ignoredBuilds: result.ignoredBuilds,
    resolutionPolicyViolations: result.resolutionPolicyViolations,
    dryRunResult: result.dryRunResult,
  }
}
