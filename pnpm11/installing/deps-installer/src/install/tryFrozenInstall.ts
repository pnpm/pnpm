import path from 'node:path'

import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { headlessInstall } from '@pnpm/installing.deps-restorer'
import { isEmptyLockfile, writeLockfiles, writeWantedLockfile } from '@pnpm/lockfile.fs'
import { allProjectsAreUpToDate } from '@pnpm/lockfile.verification'
import { logger } from '@pnpm/logger'
import type { PatchGroupRecord } from '@pnpm/patching.config'
import type { ProjectRootDir } from '@pnpm/types'

import { getCurrentEngine } from './currentEngine.js'
import { BROKEN_LOCKFILE_INTEGRITY_ERRORS } from './frozenInstallErrors.js'
import { hasUninstallMutations, isCheckOnlyInstall, pkgHasDependencies, usesSingleLockfile } from './installPredicates.js'
import { upToDateCheckOptions } from './lockfileState.js'
import type { InnerInstallResult, MutationRun, UpdatedProject } from './mutationTypes.js'
import { checkProjectsAgainstLockfile, type SkippedOptionalDependencies, verifyLockedTarballIntegrity } from './verifyFrozenLockfile.js'

export interface FrozenInstallArgs {
  /**
   * Whether every `installSome` mutation's manifest edit has been applied
   * to the context. Without it the up-to-date check would not see the
   * requested dependencies, and a frozen-like install would leave them
   * uninstalled.
   */
  addedManifestsAreCommitted: boolean
  didFastUpdateOverrides: boolean
  frozenLockfile: boolean
  projectDirsRemovingDeps: Set<ProjectRootDir>
  needsFullResolution: boolean
  patchGroups?: PatchGroupRecord
  untrackedReadPackageHookMayHaveChanged: boolean
  upToDateLockfileMajorVersion: boolean
}

/**
 * Attempt to perform a "frozen install".
 *
 * A "frozen install" will be performed if:
 *
 *   1. The --frozen-lockfile flag was explicitly specified or evaluates to
 *      true based on conditions like running on CI.
 *   2. No workspace modifications have been made that would invalidate the
 *      pnpm-lock.yaml file. In other words, the pnpm-lock.yaml file is
 *      known to be "up-to-date".
 *
 * A frozen install is significantly faster since the pnpm-lock.yaml file
 * can treated as immutable, skipping expensive lookups to acquire new
 * dependencies. For this reason, a frozen install should be performed even
 * if --frozen-lockfile wasn't explicitly specified.
 *
 * If a frozen install is not possible, this function will return null.
 * This indicates a standard mutable install needs to be performed.
 *
 * Note this function may update the pnpm-lock.yaml file if the lockfile was
 * on a different major version, needs to be merged due to git conflicts,
 * etc. These changes update the format of the pnpm-lock.yaml file, but do
 * not change recorded dependency resolutions.
 */
export async function tryFrozenInstall (
  run: MutationRun,
  args: FrozenInstallArgs
): Promise<InnerInstallResult | { needsFullResolution: boolean } | null> {
  const { ctx, opts, projects } = run
  if (!await isFrozenInstallPossible(run, args)) {
    return null
  }

  if (args.needsFullResolution) {
    throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
      'Cannot perform a frozen installation because the version of the lockfile is incompatible with this version of pnpm',
      {
        hint: `Try either:
1. Aligning the version of pnpm that generated the lockfile with the version that installs from it, or
2. Migrating the lockfile so that it is compatible with the newer version of pnpm, or
3. Using "pnpm install --no-frozen-lockfile".
Note that in CI environments, this setting is enabled by default.`,
      }
    )
  }
  // Optional dependencies the install that wrote the lockfile could not
  // resolve. A frozen install skips them again and reports each one the way
  // the resolver does. The report stays here on a delegated install too:
  // pacquet runs with `--ignore-manifest-check` and reports nothing.
  const skippedOptionalDependencies = opts.ignorePackageManifest
    ? []
    : await checkProjectsAgainstLockfile(run, args.frozenLockfile)
  reportSkippedOptionalDependencies(skippedOptionalDependencies)
  if (args.frozenLockfile && !opts.lockfileOnly) {
    await verifyLockedTarballIntegrity(run)
  }
  if (opts.lockfileOnly) {
    // The lockfile will only be changed if the workspace will have new projects with no dependencies.
    await writeWantedLockfile(ctx.lockfileDir, ctx.wantedLockfile)
    return {
      updatedProjects: projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir]),
      wantedLockfile: ctx.wantedLockfile,
      ignoredBuilds: undefined,
    }
  }
  if (isEmptyLockfile(ctx.wantedLockfile) && skippedOptionalDependencies.length === 0) {
    if (Object.values(ctx.projects).some((project) => pkgHasDependencies(project.manifest))) {
      throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
    }
    return null
  }
  return materializeFrozenLockfile(run, args)
}

async function isFrozenInstallPossible (run: MutationRun, args: FrozenInstallArgs): Promise<boolean> {
  const { ctx, installsAndUninstallsOnly, installsOnly, opts } = run
  if (ctx.lockfileHadConflicts || opts.fixLockfile || opts.dedupe) return false
  // A check-only install (`lockfileCheck`, used by `--dry-run` and
  // `dedupe --check`) must always run a full resolution so the wanted
  // lockfile can be compared, and must never materialize anything. The
  // frozen path would skip resolution and/or perform a real install.
  if (isCheckOnlyInstall(opts)) return false
  if (!installsOnly && !(installsAndUninstallsOnly && args.addedManifestsAreCommitted && !args.frozenLockfile)) return false
  if (args.frozenLockfile) return true
  return opts.ignorePackageManifest || lockfileIsUpToDateWithProjects(run, args)
}

async function lockfileIsUpToDateWithProjects (
  { ctx, opts }: MutationRun,
  { needsFullResolution, untrackedReadPackageHookMayHaveChanged }: FrozenInstallArgs
): Promise<boolean> {
  if (needsFullResolution || !opts.preferFrozenLockfile) return false
  // A `readPackage` hook the pnpmfile checksum cannot vouch for — a
  // programmatic one, or one from the checksum-excluded global
  // pnpmfile — makes "up to date" unverifiable: an edit to it leaves
  // no trace the lockfile comparison can see, so the lockfile must
  // not be trusted blindly here. (The explicit `--frozen-lockfile`
  // branch above keeps its contract: it never resolves.)
  // https://github.com/pnpm/pnpm/issues/15136
  if (untrackedReadPackageHookMayHaveChanged) return false
  if (opts.pruneLockfileImporters && Object.keys(ctx.wantedLockfile.importers).length !== Object.keys(ctx.projects).length) return false
  if (isEmptyLockfile(ctx.wantedLockfile) || ctx.wantedLockfile.lockfileVersion !== LOCKFILE_VERSION) return false
  return allProjectsAreUpToDate(Object.values(ctx.projects), upToDateCheckOptions(opts, { ctx, wantedLockfile: ctx.wantedLockfile }))
}

function reportSkippedOptionalDependencies (skippedOptionalDependencies: SkippedOptionalDependencies[]): void {
  for (const { prefix, skipped } of skippedOptionalDependencies) {
    for (const [name, bareSpecifier] of Object.entries(skipped)) {
      skippedOptionalDependencyLogger.debug({
        package: { name, version: bareSpecifier, bareSpecifier },
        parents: [],
        prefix,
        reason: 'resolution_failure',
      })
    }
  }
}

async function materializeFrozenLockfile (run: MutationRun, args: FrozenInstallArgs): Promise<InnerInstallResult | { needsFullResolution: boolean }> {
  const { maybeOpts, opts, projects } = run
  if (maybeOpts.ignorePackageManifest) {
    logger.info({ message: 'Importing packages to virtual store', prefix: opts.lockfileDir })
  } else {
    logger.info({ message: 'Lockfile is up to date, resolution step is skipped', prefix: opts.lockfileDir })
  }
  if (
    opts.runPacquet != null &&
    usesSingleLockfile(opts) &&
    !isCheckOnlyInstall(opts) &&
    opts.enableModulesDir &&
    !hasUninstallMutations(projects)
  ) {
    return runPacquetOnFrozenLockfile(run, opts.runPacquet)
  }
  try {
    return await runHeadlessInstall(run, args)
  } catch (error: unknown) {
    return recoverFromBrokenLockfile(run, { args, error: error as BrokenLockfileError })
  }
}

function listUpdatedProjects ({ ctx, projects }: MutationRun): UpdatedProject[] {
  return projects.map((mutatedProject) => {
    const project = ctx.projects[mutatedProject.rootDir]
    return {
      ...project,
      manifest: project.originalManifest ?? project.manifest,
    }
  })
}

async function runPacquetOnFrozenLockfile (
  run: MutationRun,
  runPacquet: NonNullable<MutationRun['opts']['runPacquet']>
): Promise<InnerInstallResult> {
  const updatedProjects = listUpdatedProjects(run)
  await run.opts.beforeLifecycleScripts?.({
    updatedProjects,
    updatedCatalogs: undefined,
    newLockfile: undefined,
    wantedLockfile: run.ctx.wantedLockfile,
    resolutionPolicyViolations: undefined,
  })
  try {
    await runPacquet.run({ rootProjectPreinstallRan: run.rootProjectPreinstallRan })
  } catch (err) {
    // Same reasoning as the verifyLockfileResolutions catch in `settleInstall`:
    // this is the user-facing failure path, so detach the reporter listener
    // before rethrowing so long-lived processes don't leak it.
    run.detachReporter()
    throw err
  }
  return {
    updatedProjects,
    wantedLockfile: run.ctx.wantedLockfile,
    ignoredBuilds: undefined,
  }
}

async function runHeadlessInstall (run: MutationRun, args: FrozenInstallArgs): Promise<InnerInstallResult> {
  const { ctx, opts } = run
  const { stats, ignoredBuilds } = await headlessInstall(createHeadlessOptions(run, args))
  if (
    opts.useLockfile && opts.saveLockfile && opts.mergeGitBranchLockfiles ||
    !args.upToDateLockfileMajorVersion && !opts.frozenLockfile
  ) {
    const currentLockfileDir = path.join(ctx.rootModulesDir, '.pnpm')
    await writeLockfiles({
      currentLockfile: ctx.currentLockfile,
      currentLockfileDir,
      wantedLockfile: ctx.wantedLockfile,
      wantedLockfileDir: ctx.lockfileDir,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
  }
  return {
    updatedProjects: listUpdatedProjects(run),
    wantedLockfile: ctx.wantedLockfile,
    stats,
    ignoredBuilds,
  }
}

function createHeadlessOptions (
  { ctx, maybeOpts, opts, projects, pruneVirtualStore, rootProjectPreinstallRan, verifyLockfile }: MutationRun,
  { didFastUpdateOverrides, patchGroups, projectDirsRemovingDeps }: FrozenInstallArgs
): Parameters<typeof headlessInstall>[0] {
  return {
    ...ctx,
    ...opts,
    currentEngine: getCurrentEngine(opts),
    currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
    patchedDependencies: patchGroups,
    selectedProjectDirs: projects.map((project) => project.rootDir),
    projectDirsRunningScripts: projects
      .filter((project) => project.mutation !== 'uninstallSome' && project.mutation !== 'installSome')
      .map((project) => project.rootDir),
    projectDirsRunningInstallOnlyScripts: projects
      .filter((project) => project.mutation === 'installSome')
      .map((project) => project.rootDir),
    rootProjectPreinstallRan,
    projectDirsRunningUninstallScripts: [...projectDirsRemovingDeps],
    allProjects: ctx.projects,
    prunedAt: ctx.modulesFile?.prunedAt,
    pruneVirtualStore,
    relinkChangedDependenciesOnly: didFastUpdateOverrides,
    wantedLockfile: maybeOpts.ignorePackageManifest ? undefined : ctx.wantedLockfile,
    useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
    verifyLockfile,
  }
}

type BrokenLockfileError = Error & { code: string }

function recoverFromBrokenLockfile (
  { ctx, opts }: MutationRun,
  { args, error }: { args: FrozenInstallArgs, error: BrokenLockfileError }
): { needsFullResolution: boolean } {
  const isIntegrityError = BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)
  const isBrokenLockfileError = error.code === 'ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY' || isIntegrityError
  if (
    args.frozenLockfile ||
    !isBrokenLockfileError ||
    (!ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile) ||
    (isIntegrityError && !opts.updateChecksums)
  ) throw error
  // A broken lockfile may be caused by a badly resolved Git conflict
  logger.warn({
    error,
    message: error.message,
    prefix: ctx.lockfileDir,
  })
  logger.error(new PnpmError(error.code, 'The lockfile is broken! Resolution step will be performed to fix it.'))
  return { needsFullResolution: args.needsFullResolution }
}
