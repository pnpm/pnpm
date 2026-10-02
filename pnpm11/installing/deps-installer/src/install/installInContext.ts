import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { PnpmContext } from '@pnpm/installing.context'
import { getWantedDependencies } from '@pnpm/installing.deps-resolver'
import { headlessInstall, type InstallationResultStats } from '@pnpm/installing.deps-restorer'
import { readEnvLockfile, readWantedLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import type { IgnoredBuilds } from '@pnpm/types'
import { isSubdir } from 'is-subdir'

import { getCurrentEngine } from './currentEngine.js'
import { forgetResolutionsOfPrevWantedDeps } from './forgetResolutions.js'
import { BROKEN_LOCKFILE_INTEGRITY_ERRORS } from './frozenInstallErrors.js'
import {
  hasUninstallMutations,
  isCheckOnlyInstall,
  materializesGroupSubset,
  pacquetResolvesInstall,
  usesSingleLockfile,
} from './installPredicates.js'
import type {
  ImporterToUpdate,
  InstallFunctionResult,
  InstallInContextOptions,
  MutatedProject,
} from './mutationTypes.js'
import { resolveAndLinkProjects } from './resolveAndLinkProjects.js'

type RunPacquet = NonNullable<InstallInContextOptions['runPacquet']>

type EnvLockfile = NonNullable<Awaited<ReturnType<typeof readEnvLockfile>>>

export async function installInContext (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Promise<InstallFunctionResult> {
  try {
    return await dispatchInstall(projects, ctx, opts)
  } catch (error: unknown) {
    return retryWithRefreshedChecksums(projects, ctx, { error: error as Error & { code: string }, opts })
  } finally {
    await opts.storeController.close()
  }
}

async function dispatchInstall (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Promise<InstallFunctionResult> {
  if (!opts.frozenLockfile && opts.useLockfile) {
    const workspaceProjects = listWorkspaceProjectsOutsideInstall(projects, ctx, opts)
    if (workspaceProjects != null) {
      return installWithWorkspaceProjects(projects, ctx, { opts, workspaceProjects })
    }
  }
  // Both a hoisted linker and a group-filtered install resolve first and
  // materialize from the filtered lockfile afterwards. Not a group filter
  // pacquet resolves under, though: it applies the filter to its own fetch,
  // and intercepting the install here would take away the resolution
  // `mutateModules` waived the lockfile verification for.
  if (resolvesBeforeMaterializing(projects, opts)) {
    return resolveThenMaterialize(projects, ctx, opts)
  }
  // Isolated `nodeLinker` (the default) with a non-frozen install.
  // The frozen branch is handled earlier in `tryFrozenInstall`; the
  // branch above runs a resolve-then-materialize sequence.
  if (opts.runPacquet != null && pacquetMaterializesInstall(projects, opts)) {
    return installWithPacquet(projects, ctx, { opts, runPacquet: opts.runPacquet })
  }
  return resolveAndLinkProjects(projects, ctx, opts)
}

function resolvesBeforeMaterializing (projects: ImporterToUpdate[], opts: InstallInContextOptions): boolean {
  return (opts.nodeLinker === 'hoisted' || (materializesGroupSubset(opts.include, projects) && !pacquetResolvesInstall(projects, opts))) &&
    !opts.lockfileOnly && !isCheckOnlyInstall(opts) && opts.enableModulesDir
}

function pacquetMaterializesInstall (projects: ImporterToUpdate[], opts: InstallInContextOptions): boolean {
  const writesSingleLockfile = opts.saveLockfile && usesSingleLockfile(opts)
  return writesSingleLockfile &&
    !opts.lockfileOnly &&
    !isCheckOnlyInstall(opts) &&
    opts.enableModulesDir &&
    !hasUninstallMutations(projects)
}

async function retryWithRefreshedChecksums (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  { error, opts }: { error: Error & { code: string }, opts: InstallInContextOptions }
): Promise<InstallFunctionResult> {
  if (
    !BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code) ||
    (!ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile) ||
    !opts.updateChecksums
  ) throw error
  opts.needsFullResolution = true
  logger.warn({
    error,
    message: error.message,
    prefix: ctx.lockfileDir,
  })
  logger.error(new PnpmError(error.code, 'Refreshing the locked integrity from the registry as requested by --update-checksums. A full installation will be performed.'))
  return resolveAndLinkProjects(projects, ctx, opts)
}

/**
 * The workspace projects this install does not select, when resolving has to
 * include them. `undefined` when the selected projects are the whole workspace.
 */
function listWorkspaceProjectsOutsideInstall (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Array<PnpmContext['projects'][string]> | undefined {
  const isPathInsideWorkspace = isSubdir.bind(null, opts.lockfileDir)
  const allProjectsLocatedInsideWorkspace = Object.values(ctx.projects)
    .filter((project) => isPathInsideWorkspace(project.rootDir))
  if (allProjectsLocatedInsideWorkspace.length > projects.length && !isCheckOnlyInstall(opts) && opts.enableModulesDir) {
    return allProjectsLocatedInsideWorkspace
  }
  return undefined
}

async function installWithWorkspaceProjects (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  { opts, workspaceProjects }: { opts: InstallInContextOptions, workspaceProjects: Array<PnpmContext['projects'][string]> }
): Promise<InstallFunctionResult> {
  const newProjects = addUnselectedWorkspaceProjects(projects, ctx, { opts, workspaceProjects })
  if (opts.lockfileOnly) return resolveAndLinkProjects(newProjects, ctx, opts)
  const result = await installInContext(newProjects, ctx, {
    ...opts,
    lockfileOnly: true,
  })
  const { stats, ignoredBuilds } = await materializeResolvedLockfile(projects, ctx, {
    mutatedProjects: newProjects,
    opts,
    result,
  })
  return {
    ...result,
    stats,
    ignoredBuilds,
  }
}

function addUnselectedWorkspaceProjects (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  { opts, workspaceProjects }: { opts: InstallInContextOptions, workspaceProjects: Array<PnpmContext['projects'][string]> }
): ImporterToUpdate[] {
  const newProjects = [...projects]
  const getWantedDepsOpts = {
    autoInstallPeers: opts.autoInstallPeers,
    includeDirect: opts.includeDirect,
    updateWorkspaceDependencies: false,
    injectWorkspacePackages: opts.injectWorkspacePackages,
  }
  for (const project of workspaceProjects) {
    if (newProjects.some(({ rootDir }) => rootDir === project.rootDir)) continue
    // This code block mirrors the installCase() function in
    // mutateModules(). Consider a refactor that combines this logic to
    // deduplicate code.
    const wantedDependencies = getWantedDependencies(project.manifest, getWantedDepsOpts)
      .map((wantedDependency) => ({ ...wantedDependency, updateSpec: true, preserveNonSemverVersionSpec: true }))
    forgetResolutionsOfPrevWantedDeps(wantedDependencies, {
      importer: ctx.wantedLockfile.importers[project.id],
      prevCatalogs: ctx.wantedLockfile.catalogs,
      catalogsConfig: opts.catalogs,
    })
    newProjects.push({
      mutation: 'install',
      ...project,
      wantedDependencies,
      pruneDirectDependencies: false,
      updatePackageManifest: false,
    })
  }
  return newProjects
}

async function resolveThenMaterialize (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Promise<InstallFunctionResult> {
  const result = await resolveAndLinkProjects(projects, ctx, {
    ...opts,
    lockfileOnly: true,
    omitSummaryLog: true,
    materializeAfterResolution: true,
  })
  const { stats, ignoredBuilds } = await materializeResolvedLockfile(projects, ctx, {
    // The resolve pass above already reported the whole graph as resolved.
    omitResolvedProgress: true,
    mutatedProjects: projects,
    opts,
    result,
  })
  return {
    ...result,
    stats,
    ignoredBuilds,
  }
}

/**
 * Hands the lockfile a `lockfileOnly` resolve pass wrote to pacquet or to
 * the headless installer, after the `beforeLifecycleScripts` hook.
 */
async function materializeResolvedLockfile (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  { mutatedProjects, omitResolvedProgress, opts, result }: {
    mutatedProjects: ImporterToUpdate[]
    omitResolvedProgress?: boolean
    opts: InstallInContextOptions
    result: InstallFunctionResult
  }
): Promise<{ stats?: InstallationResultStats, ignoredBuilds?: IgnoredBuilds }> {
  const updatedProjects = mutatedProjectsToUpdatedProjects(mutatedProjects)
  await opts.beforeLifecycleScripts?.({
    updatedProjects,
    updatedCatalogs: result.updatedCatalogs,
    newLockfile: result.newLockfile,
    resolutionPolicyViolations: result.resolutionPolicyViolations,
  })
  return materializeOrDelegate(opts, async () => headlessInstall({
    ...ctx,
    ...opts,
    ...(omitResolvedProgress ? { omitResolvedProgress } : {}),
    currentEngine: getCurrentEngine(opts),
    currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
    selectedProjectDirs: projects.map((project) => project.rootDir),
    projectDirsRunningScripts: projects
      .filter((project) => project.mutation !== 'uninstallSome' && project.mutation !== 'installSome')
      .map((project) => project.rootDir),
    projectDirsRunningInstallOnlyScripts: projects
      .filter((project) => project.mutation === 'installSome')
      .map((project) => project.rootDir),
    projectDirsRunningUninstallScripts: [...opts.projectDirsRemovingDeps],
    allProjects: ctx.projects,
    prunedAt: ctx.modulesFile?.prunedAt,
    wantedLockfile: result.newLockfile,
    useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
    hoistWorkspacePackages: opts.hoistWorkspacePackages,
  }), mutatedProjects)
}

function mutatedProjectsToUpdatedProjects (projects: ImporterToUpdate[]): InstallFunctionResult['projects'] {
  return projects.map(({ manifest, originalManifest, rootDir }) => ({
    originalManifest,
    manifest,
    peerDependencyIssues: undefined,
    rootDir,
  }))
}

async function installWithPacquet (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  { opts, runPacquet }: { opts: InstallInContextOptions, runPacquet: RunPacquet }
): Promise<InstallFunctionResult> {
  // pacquet >= 0.11.7 resolves itself: hand it the whole install
  // (resolve + fetch + import + link + build, writing the lockfile)
  // in a single non-frozen pass. Only for plain installs — `add` /
  // `update` / `remove` need pnpm to mutate the manifests and
  // resolve the new specs first (pacquet's `install` reads
  // package.json from disk, which pnpm hasn't rewritten yet).
  if (pacquetResolvesInstall(projects, opts)) {
    await runPacquetResolution(ctx, { opts, runPacquet })
    ctx.wantedLockfile = await readLockfileWrittenByPacquet(ctx, opts)
    return pacquetResolveResult(projects, ctx)
  }
  // Older pacquet can only materialize: split the install in two —
  // ask `resolveAndLinkProjects` for a `lockfileOnly` resolve pass (writes
  // `pnpm-lock.yaml`), then hand the freshly-written lockfile to
  // pacquet for the fetch / import / link / build phases.
  const result = await resolveAndLinkProjects(projects, ctx, { ...opts, lockfileOnly: true })
  await opts.beforeLifecycleScripts?.({
    updatedProjects: result.projects,
    updatedCatalogs: result.updatedCatalogs,
    newLockfile: result.newLockfile,
    resolutionPolicyViolations: result.resolutionPolicyViolations,
  })
  await runPacquet.run({ filterResolvedProgress: true, rootProjectPreinstallRan: opts.rootProjectPreinstallRan })
  return result
}

/**
 * `configDependencies` are recorded in a YAML document prepended
 * to `pnpm-lock.yaml` — purely a pnpm concept that pacquet doesn't
 * model. Capture it before pacquet rewrites the lockfile and
 * restore it afterwards (`writeEnvLockfile` re-reads pacquet's main
 * document and re-prepends the env document), otherwise the next
 * `--frozen-lockfile` install fails its config-deps freshness gate.
 * The restore runs even if pacquet fails partway: a non-zero exit can
 * still leave a rewritten lockfile behind, so the env document must be
 * put back regardless.
 */
async function runPacquetResolution (
  ctx: PnpmContext,
  { opts, runPacquet }: { opts: InstallInContextOptions, runPacquet: RunPacquet }
): Promise<void> {
  const envLockfile = await readEnvLockfile(ctx.lockfileDir)
  let pacquetError: unknown
  try {
    await runPacquet.run({ resolve: true, rootProjectPreinstallRan: opts.rootProjectPreinstallRan })
  } catch (err: unknown) {
    pacquetError = err
    throw err
  } finally {
    if (envLockfile != null) {
      await restoreEnvLockfile(ctx.lockfileDir, { envLockfile, pacquetError })
    }
  }
}

async function restoreEnvLockfile (
  lockfileDir: string,
  { envLockfile, pacquetError }: { envLockfile: EnvLockfile, pacquetError: unknown }
): Promise<void> {
  await writeEnvLockfile(lockfileDir, envLockfile).catch((restoreErr: Error) => {
    if (pacquetError == null) {
      throw restoreErr
    }
    logger.warn({
      error: restoreErr,
      message: `Failed to restore the configDependencies document in pnpm-lock.yaml: ${restoreErr.message}`,
      prefix: lockfileDir,
    })
  })
}

async function readLockfileWrittenByPacquet (
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Promise<PnpmContext['wantedLockfile']> {
  const wantedLockfile = await readWantedLockfile(ctx.lockfileDir, {
    ignoreIncompatible: opts.force || opts.ci === true,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    wantedVersions: [LOCKFILE_VERSION],
  })
  if (wantedLockfile == null) {
    throw new PnpmError('PACQUET_LOCKFILE_READ_FAILED', `pacquet did not write a readable ${WANTED_LOCKFILE}`)
  }
  return wantedLockfile
}

/**
 * The `InstallFunctionResult` for an install pacquet resolved and
 * materialized end-to-end. pacquet wrote `pnpm-lock.yaml` and the
 * `node_modules` tree itself. `ctx.wantedLockfile` has already been
 * refreshed from disk. Resolution-policy handlers are guarded out before
 * this path, so there are no command-layer policy violations to return.
 * Manifests are returned unchanged — this path only runs for plain
 * installs, which don't rewrite `package.json`.
 */
function pacquetResolveResult (projects: ImporterToUpdate[], ctx: PnpmContext): InstallFunctionResult {
  return {
    newLockfile: ctx.wantedLockfile,
    projects: projects.map((project) => ({
      manifest: project.originalManifest ?? project.manifest,
      rootDir: project.rootDir,
    })),
    depsRequiringBuild: [],
    resolutionPolicyViolations: [],
  }
}

/**
 * Run the pacquet binary if it's configured, otherwise run the JS
 * `headlessInstall`. Callers can hand off any code path that materializes
 * an already-resolved lockfile without restating the delegation choice.
 *
 * Pacquet reads the wanted lockfile from disk and produces its own
 * `pnpm:stats` / `pnpm:ignored-scripts` log events that drive the
 * reporter. The structured stats / ignoredBuilds return values that
 * `headlessInstall` produces aren't recovered here — pacquet doesn't
 * surface them through any return path — so callers get `undefined` for
 * both. `mutateModules` already tolerates that (it falls back to a zero
 * stats record and a no-op ignoredBuilds iteration).
 */
export async function materializeOrDelegate (
  opts: {
    mergeGitBranchLockfiles?: boolean
    rootProjectPreinstallRan?: boolean
    runPacquet?: { run: (opts?: { filterResolvedProgress?: boolean, rootProjectPreinstallRan?: boolean }) => Promise<void> }
    saveLockfile?: boolean
    useGitBranchLockfile?: boolean
    useLockfile?: boolean
  },
  runHeadlessInstall: () => Promise<{ stats: InstallationResultStats, ignoredBuilds: IgnoredBuilds | undefined }>,
  projects?: MutatedProject[]
): Promise<{ stats?: InstallationResultStats, ignoredBuilds?: IgnoredBuilds }> {
  if (
    opts.runPacquet != null &&
    usesSingleLockfile(opts) &&
    opts.saveLockfile !== false &&
    (projects == null || !hasUninstallMutations(projects))
  ) {
    // The callers ran a lockfileOnly resolve pass that emitted one
    // `pnpm:progress status:resolved` per package, so pacquet's
    // duplicate `resolved` events would double the reporter's count.
    await opts.runPacquet.run({ filterResolvedProgress: true, rootProjectPreinstallRan: opts.rootProjectPreinstallRan })
    return {}
  }
  return runHeadlessInstall()
}
