import path from 'node:path'

import { UNDECIDED_ALLOW_BUILD } from '@pnpm/building.policy'
import { mergeCatalogs } from '@pnpm/catalogs.config'
import type { Catalogs } from '@pnpm/catalogs.types'
import { type RecursiveSummary, throwOnCommandFail } from '@pnpm/cli.utils'
import {
  binDirOf,
  type Config,
  getWorkspaceConcurrency,
  type OptionsFromRootManifest,
  type ProjectConfig,
} from '@pnpm/config.reader'
import { requireHooks } from '@pnpm/hooks.pnpmfile'
import {
  addDependenciesToPackage,
  install,
  type InstallOptions,
  mutateModules,
} from '@pnpm/installing.deps-installer'
import { logger } from '@pnpm/logger'
import type {
  DepPath,
  IgnoredBuilds,
  PackageManifest,
  Project,
  ProjectManifest,
  ProjectRootDir,
  RangeSpecStyle,
} from '@pnpm/types'
import { syncInjectedDepsOfModulesDir } from '@pnpm/workspace.injected-deps-syncer'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

import { handleIgnoredBuilds } from '../handleIgnoredBuilds.js'
import type { PolicyViolation } from '../policyHandlers.js'
import { resolvedPackageVersionsOfProjectLockfiles } from '../resolvedPackageVersionsForPrune.js'
import {
  completeDependencySelectors,
  createNoPackageInDependenciesError,
  getProjectRangeSpecStyle,
  type ManifestsByPath,
  type RecursiveContext,
  selectUpdateTargets,
} from './context.js'
import type { CommandFullName, RecursiveOptions, RecursiveResult } from './options.js'

type ActionOpts =
  & Omit<InstallOptions, 'allProjects'>
  & OptionsFromRootManifest
  & Project
  & Pick<Config, 'bin'>
  & { rangeSpecStyle: RangeSpecStyle }

interface ActionResult {
  updatedCatalogs?: Catalogs
  updatedManifest: ProjectManifest
  ignoredBuilds: IgnoredBuilds | undefined
  resolutionPolicyViolations?: PolicyViolation[]
}

type ActionFunction = (manifest: PackageManifest | ProjectManifest, opts: ActionOpts) => Promise<ActionResult>

/** What the per-project installs of one run collect for the steps after them. */
interface ProjectInstallsState {
  result: RecursiveSummary
  buildsAfterInstall: boolean
  updatedCatalogs?: Catalogs
  allIgnoredBuilds: Set<DepPath>
  /**
   * Each per-project install returns its own slice of lockfile-resolution
   * violations; they accumulate here so the post-loop persist step can
   * dedup and write a single batch to the workspace manifest.
   */
  allResolutionPolicyViolations: PolicyViolation[]
  installedModulesDirs: Map<ProjectRootDir, string>
  firstError?: Error
}

/**
 * Installs each selected project of a workspace without a shared lockfile on
 * its own, in dependency order.
 */
export async function installEachProject (ctx: RecursiveContext): Promise<RecursiveResult> {
  const { opts, cmdFullName } = ctx
  const pkgPaths = (Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]).sort()
  const selectedProjectDependencies = opts.sort !== false
    ? filteredProjectsDependencies(opts)
    : new Map(pkgPaths.map((rootDir) => [rootDir, []]))
  const state: ProjectInstallsState = {
    result: {},
    allIgnoredBuilds: new Set<DepPath>(),
    allResolutionPolicyViolations: [],
    installedModulesDirs: new Map<ProjectRootDir, string>(),
    buildsAfterInstall: !opts.lockfileOnly && !opts.ignoreScripts &&
      (cmdFullName === 'add' || cmdFullName === 'install' || cmdFullName === 'update'),
  }
  await scheduleGraph(selectedProjectDependencies, {
    bail: opts.bail !== false,
    concurrency: getWorkspaceConcurrency(opts.workspaceConcurrency),
    continueOnFailure: opts.bail === false,
    runNode: async (rootDir) => runProjectInstall(ctx, state, rootDir),
    onNodeSkipped: () => {},
  })
  if (state.firstError != null) throw state.firstError
  await handleIgnoredBuilds(opts, state.allIgnoredBuilds.size ? state.allIgnoredBuilds : undefined)
  if (opts.save !== false) {
    await updateWorkspaceManifestAfterInstalls(ctx, state)
  }
  if (state.buildsAfterInstall) {
    await buildInstalledProjects(ctx, state.installedModulesDirs)
  }
  throwOnCommandFail(`pnpm recursive ${cmdFullName}`, state.result)
  if (!hasPassedProject(state.result) && cmdFullName === 'update' && opts.depth === 0) {
    throw createNoPackageInDependenciesError()
  }
  return { passed: true, updatedCatalogs: state.updatedCatalogs }
}

function hasPassedProject (result: RecursiveSummary): boolean {
  return Object.values(result).filter(({ status }) => status === 'passed').length > 0
}

async function runProjectInstall (
  ctx: RecursiveContext,
  state: ProjectInstallsState,
  rootDir: ProjectRootDir
): Promise<TaskCompletion> {
  try {
    return await installProject(ctx, state, rootDir)
  } catch (err: any) { // eslint-disable-line
    logger.info(err)

    if (!ctx.opts.bail) {
      state.result[rootDir] = {
        status: 'failure',
        error: err,
        message: err.message,
        prefix: rootDir,
      }
      return 'failed'
    }

    err['prefix'] = rootDir
    state.firstError ??= err
    return 'aborted'
  }
}

async function installProject (
  ctx: RecursiveContext,
  state: ProjectInstallsState,
  rootDir: ProjectRootDir
): Promise<TaskCompletion> {
  if (ctx.opts.ignoredPackages?.has(rootDir)) {
    state.result[rootDir] = { status: 'skipped' }
    return 'passed'
  }
  state.result[rootDir] = { status: 'running' }
  const hooks = await loadProjectHooks(ctx.opts, rootDir)
  const { manifest, writeProjectManifest } = ctx.manifestsByPath[rootDir]
  const updateTargets = selectUpdateTargets(ctx, manifest)
  if (ctx.updateMatch != null && updateTargets.length === 0) {
    state.result[rootDir] = { status: 'skipped' }
    return 'passed'
  }
  const action = createProjectAction(ctx.cmdFullName, {
    rootDir,
    dependencySelectors: completeDependencySelectors(ctx, manifest, updateTargets),
  })
  const localConfig = ctx.getProjectConfig(manifest) ?? {}
  const actionResult = await action(manifest, {
    ...ctx.installOpts,
    ...localConfig,
    ...ctx.opts.allProjectsGraph[rootDir]?.package,
    bin: binDirOf(rootDir, localConfig.modulesDir ?? ctx.opts.modulesDir),
    dir: rootDir,
    hooks,
    deferDependencyBuilds: state.buildsAfterInstall,
    ignoreScripts: true,
    rangeSpecStyle: getProjectRangeSpecStyle(ctx.opts, localConfig),
    configByUri: ctx.installOpts.configByUri,
    storeController: ctx.store.ctrl,
    resolutionVerifiers: ctx.store.resolutionVerifiers,
  })
  await recordProjectInstall(ctx, state, { actionResult, localConfig, rootDir, writeProjectManifest })
  state.result[rootDir].status = 'passed'
  return 'passed'
}

async function loadProjectHooks (opts: RecursiveOptions, rootDir: ProjectRootDir): Promise<InstallOptions['hooks']> {
  if (opts.ignorePnpmfile) return {}
  const { hooks: pnpmfileHooks } = await requireHooks(rootDir, opts)
  return {
    ...opts.hooks,
    ...pnpmfileHooks,
    afterAllResolved: [...(pnpmfileHooks.afterAllResolved ?? []), ...(opts.hooks?.afterAllResolved ?? [])],
    readPackage: [...(pnpmfileHooks.readPackage ?? []), ...(opts.hooks?.readPackage ?? [])],
  }
}

interface ProjectActionTarget {
  rootDir: ProjectRootDir
  dependencySelectors: string[]
}

function createProjectAction (
  cmdFullName: CommandFullName,
  { rootDir, dependencySelectors }: ProjectActionTarget
): ActionFunction {
  if (cmdFullName === 'remove') {
    return async (_manifest, opts) => {
      const mutationResult = await mutateModules([
        {
          dependencyNames: dependencySelectors,
          mutation: 'uninstallSome',
          rootDir,
        },
      ], opts)
      return {
        updatedCatalogs: undefined, // there's no reason to add new or update catalogs on `pnpm remove`
        updatedManifest: mutationResult.updatedProjects[0].manifest,
        ignoredBuilds: mutationResult.ignoredBuilds,
        resolutionPolicyViolations: mutationResult.resolutionPolicyViolations,
      }
    }
  }
  return dependencySelectors.length === 0
    ? install
    : async (manifest, opts) => addDependenciesToPackage(manifest, dependencySelectors, opts)
}

interface InstalledProject {
  actionResult: ActionResult
  localConfig: ProjectConfig
  rootDir: ProjectRootDir
  writeProjectManifest: Project['writeProjectManifest']
}

async function recordProjectInstall (
  { opts }: RecursiveContext,
  state: ProjectInstallsState,
  { actionResult, localConfig, rootDir, writeProjectManifest }: InstalledProject
): Promise<void> {
  if (opts.save !== false) {
    await writeProjectManifest(actionResult.updatedManifest)
    if (actionResult.updatedCatalogs) {
      // Per-project additions are partial maps keyed by catalog name then
      // dependency. Merge at the dependency level so two projects updating
      // different entries of the same catalog don't clobber each other.
      state.updatedCatalogs = mergeCatalogs(state.updatedCatalogs, actionResult.updatedCatalogs)
    }
  }
  for (const depPath of actionResult.ignoredBuilds ?? []) {
    state.allIgnoredBuilds.add(depPath)
  }
  for (const violation of actionResult.resolutionPolicyViolations ?? []) {
    state.allResolutionPolicyViolations.push(violation)
  }
  state.installedModulesDirs.set(rootDir, path.resolve(rootDir, localConfig.modulesDir ?? opts.modulesDir ?? 'node_modules'))
}

async function updateWorkspaceManifestAfterInstalls (
  { allProjects, opts, policyHandlers }: RecursiveContext,
  state: ProjectInstallsState
): Promise<void> {
  // Only pick entries when we'll actually persist. Otherwise the
  // info log would claim entries were added that the workspace
  // manifest never saw, mirroring the gate the shared-lockfile
  // branch + installDeps already apply.
  // Only a run that installed every workspace project leaves no lockfile
  // behind its manifest; a filtered or partly skipped run prunes nothing.
  const everyProjectInstalled = allProjects.every(({ rootDir }) => state.result[rootDir]?.status === 'passed')
  const needsResolvedPackageVersions = Boolean(
    opts.minimumReleaseAgeExcludePrune ||
    opts.trustPolicyExcludePrune ||
    Object.values(opts.allowBuilds ?? {}).includes(UNDECIDED_ALLOW_BUILD)
  )
  await updateWorkspaceManifest(opts.workspaceDir, {
    updatedCatalogs: state.updatedCatalogs,
    catalogPrune: opts.catalogPrune,
    resolvedPackageVersions: everyProjectInstalled && !opts.dryRun && needsResolvedPackageVersions
      ? await resolvedPackageVersionsOfProjectLockfiles(opts, allProjects.map(({ rootDir }) => rootDir))
      : undefined,
    minimumReleaseAgeExcludePrune: opts.minimumReleaseAgeExcludePrune,
    trustPolicyExcludePrune: opts.trustPolicyExcludePrune,
    allProjects,
    ...policyHandlers?.pickManifestUpdates(state.allResolutionPolicyViolations),
  })
}

async function buildInstalledProjects (
  { manifestsByPath, opts }: RecursiveContext,
  installedModulesDirs: Map<ProjectRootDir, string>
): Promise<void> {
  await opts.rebuildHandler?.({
    ...opts,
    pending: opts.pending === true,
    skipIfHasSideEffectsCache: true,
  }, [])
  // With a shared lockfile, an injected project is imported again after its
  // own lifecycle scripts run. Here each project was installed and built on
  // its own, so the copies are synced once every project has been built.
  if (!opts.dryRun) {
    await syncInjectedDepsOfInstalledProjects(manifestsByPath, installedModulesDirs, !opts.deployAllFiles)
  }
}

async function syncInjectedDepsOfInstalledProjects (
  manifestsByPath: ManifestsByPath,
  installedModulesDirs: Map<ProjectRootDir, string>,
  includeOnlyPackageFiles: boolean
): Promise<void> {
  const injectedSourceDirs = collectInjectedSourceDirs(manifestsByPath, installedModulesDirs.keys())
  const syncResults = await Promise.allSettled(Array.from(installedModulesDirs, async ([lockfileDir, modulesDir]) =>
    syncInjectedDepsOfModulesDir({ includeOnlyPackageFiles, lockfileDir, modulesDir, sourceDirs: injectedSourceDirs })
  ))
  const syncFailure = syncResults.find((syncResult): syncResult is PromiseRejectedResult => syncResult.status === 'rejected')
  if (syncFailure != null) throw syncFailure.reason
}

/**
 * A project that publishes from `publishConfig.directory` is injected from
 * that directory rather than from its root.
 */
function collectInjectedSourceDirs (manifestsByPath: ManifestsByPath, rootDirs: Iterable<ProjectRootDir>): Set<string> {
  const injectedSourceDirs = new Set<string>()
  for (const rootDir of rootDirs) {
    injectedSourceDirs.add(rootDir)
    const publishConfig = manifestsByPath[rootDir]?.manifest.publishConfig
    if (publishConfig?.directory != null && publishConfig.linkDirectory !== false) {
      injectedSourceDirs.add(path.resolve(rootDir, publishConfig.directory))
    }
  }
  return injectedSourceDirs
}
