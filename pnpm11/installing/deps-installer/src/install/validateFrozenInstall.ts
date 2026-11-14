import { createAllowBuildFunction } from '@pnpm/building.policy'
import { LOCKFILE_MAJOR_VERSION } from '@pnpm/constants'
import { getContext, type PnpmContext } from '@pnpm/installing.context'
import type { LockfileObject } from '@pnpm/lockfile.fs'

import { extendOptions } from './extendInstallOptions.js'
import { allMutationsAreInstalls, isCheckOnlyInstall, pkgHasDependencies } from './installPredicates.js'
import { assertFrozenPnprPnpmfileChecksum, assertPnprCompatibleWithStore } from './installViaPnprServer.js'
import { detectOutdatedLockfileSettings, readInstallChecksums } from './lockfileState.js'
import {
  createScriptsOptions,
  readMutationProjectOptions,
  runPreResolutionHooks,
  startLockfileVerification,
} from './mutateModules.js'
import type { MutatedProject, MutateModulesOptions, MutationRun } from './mutationTypes.js'
import { canUsePnprForMutations, pnpmfileHookPnprCannotRun } from './pnpr.js'
import { requiresFullResolution, throwOnInconsistentFrozenPatchHashes } from './runMutations.js'
import { createOutdatedLockfileMajorVersionError } from './tryFrozenInstall.js'
import {
  checkProjectsAgainstLockfile,
  createNoLockfileError,
  verifyLockedTarballIntegrity,
} from './verifyFrozenLockfile.js'

/**
 * Runs every read-only check a `--frozen-lockfile` install enforces before
 * `pnpm ci` cleans `node_modules`.
 */
export async function validateFrozenInstall (
  projects: MutatedProject[],
  maybeOpts: MutateModulesOptions
): Promise<LockfileObject | undefined> {
  const opts = extendOptions({
    ...maybeOpts,
    frozenLockfile: true,
    frozenLockfileIfExists: false,
  })
  if (opts.fixLockfile || isCheckOnlyInstall(opts)) return undefined
  const usesPnpr = Boolean(maybeOpts.allProjects?.length) &&
    Boolean(opts.pnprServer) &&
    canUsePnprForMutations(projects, opts) &&
    pnpmfileHookPnprCannotRun(opts.hooks) == null
  if (usesPnpr) {
    assertPnprCompatibleWithStore(opts)
  }
  if (!maybeOpts.allProjects?.length) {
    opts.allProjects = await readMutationProjectOptions(projects, opts.ignorePackageManifest)
  }
  const ctx = await getContext({ ...opts, frozenStore: true })
  assertProjectManifestsExist(ctx, Boolean(maybeOpts.ignorePackageManifest))
  if (!ctx.existsWantedLockfile) {
    if (Object.values(ctx.projects).some((project) => pkgHasDependencies(project.manifest))) {
      throw createNoLockfileError()
    }
    return undefined
  }
  if (usesPnpr) {
    assertFrozenPnprPnpmfileChecksum(
      opts,
      ctx.wantedLockfile.pnpmfileChecksum,
      await opts.hooks.calculatePnpmfileChecksum?.()
    )
  }
  if (ctx.lockfileHadConflicts) return ctx.wantedLockfile
  return verifyFrozenMutationRun(ctx, { maybeOpts, opts, projects })
}

function assertProjectManifestsExist (ctx: PnpmContext, ignorePackageManifest: boolean): void {
  if (ignorePackageManifest) return
  for (const { manifest, rootDir } of Object.values(ctx.projects)) {
    if (!manifest) {
      throw new Error(`No package.json found in "${rootDir}"`)
    }
  }
}

async function verifyFrozenMutationRun (
  ctx: PnpmContext,
  { maybeOpts, opts, projects }: Pick<MutationRun, 'maybeOpts' | 'opts' | 'projects'>
): Promise<LockfileObject> {
  const installsOnly = allMutationsAreInstalls(projects)
  const { verifyLockfilePromise } = await startLockfileVerification({ ctx, installsOnly, opts, projects })
  await runPreResolutionHooks(opts, { ...ctx, existsCurrentLockfile: false })
  const run = buildValidationRun(ctx, { installsOnly, maybeOpts, opts, projects, verifyLockfilePromise })
  await verifyFrozenLockfileState(run)
  if (verifyLockfilePromise != null) {
    await verifyLockfilePromise
  }
  return ctx.wantedLockfile
}

function buildValidationRun (
  ctx: PnpmContext,
  { installsOnly, maybeOpts, opts, projects, verifyLockfilePromise }: {
    installsOnly: boolean
    maybeOpts: MutateModulesOptions
    opts: MutationRun['opts']
    projects: MutatedProject[]
    verifyLockfilePromise?: Promise<void>
  }
): MutationRun {
  return {
    allowBuild: createAllowBuildFunction(opts),
    ctx,
    detachReporter: () => {},
    forceResolutionFromHook: false,
    installsAndUninstallsOnly: true,
    installsOnly,
    maybeOpts,
    opts,
    projects,
    pruneVirtualStore: false,
    rootProjectPreinstallRan: false,
    scriptsOpts: createScriptsOptions(opts, ctx),
    verifyLockfilePromise,
    verifyLockfile: verifyLockfilePromise && (() => verifyLockfilePromise),
  }
}

async function verifyFrozenLockfileState (run: MutationRun): Promise<void> {
  const checksums = await readInstallChecksums(run)
  const settings = detectOutdatedLockfileSettings(run, checksums)
  throwOnInconsistentFrozenPatchHashes(run.ctx, true)
  const upToDateLockfileMajorVersion = run.ctx.wantedLockfile.lockfileVersion
    .toString()
    .startsWith(`${LOCKFILE_MAJOR_VERSION}.`)
  if (requiresFullResolution(run, {
    outdatedLockfileSettings: settings.changedLockfileSettings.length > 0,
    upToDateLockfileMajorVersion,
  })) {
    throw createOutdatedLockfileMajorVersionError()
  }
  if (!run.opts.ignorePackageManifest) {
    await checkProjectsAgainstLockfile(run, true)
  }
  if (!run.opts.lockfileOnly) {
    await verifyLockedTarballIntegrity(run)
  }
}
