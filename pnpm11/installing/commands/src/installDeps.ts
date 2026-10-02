import path from 'node:path'

import { createAllowBuildFunction, unapprovedIgnoredBuilds } from '@pnpm/building.policy'
import type { CommandHandler } from '@pnpm/cli.command'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { checkDepsStatus, findDanglingDirectDependencyLink } from '@pnpm/deps.status'
import { PnpmError } from '@pnpm/error'
import {
  type DryRunInstallResult,
  IgnoredBuildsError,
  type UpdateMatchingFunction,
} from '@pnpm/installing.deps-installer'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { globalInfo, logger } from '@pnpm/logger'
import { applyRuntimeOnFailOverride } from '@pnpm/pkg-manifest.utils'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type {
  IncludedDependencies,
  PackageVulnerabilityAudit,
  Project,
  ProjectsGraph,
} from '@pnpm/types'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { sequenceGraph } from '@pnpm/workspace.projects-sorter'

import { installSingleProject } from './installSingleProject.js'
import { makeRunPacquet, type MakeRunPacquetOpts } from './runPacquet.js'
import { verifyPacquetIdentity } from './verifyPacquetIdentity.js'
import { preferNonvulnerablePackageVersions } from './vulnerabilityPreferences.js'
import { warnAboutNestedWorkspaceManifests } from './warnAboutNestedWorkspaceManifests.js'
import { recursiveInstallThenUpdateWorkspaceState } from './workspaceStateUpdate.js'

export { createVulnerabilityUpdateMatching } from './vulnerabilityPreferences.js'

export type InstallDepsOptions = Pick<Config,
| 'autoInstallPeers'
| 'bail'
| 'bin'
| 'catalogs'
| 'catalogMode'
| 'catalogPrune'
| 'minimumReleaseAgeExcludePrune'
| 'trustPolicyExcludePrune'
| 'dedupePeerDependents'
| 'dedupePeers'
| 'depth'
| 'dev'
| 'enableGlobalVirtualStore'
| 'virtualStoreOnly'
| 'engineStrict'
| 'excludeLinksFromLockfile'
| 'forceIgnoresPlatform'
| 'global'
| 'globalPnpmfile'
| 'ignoreCurrentSpecifiers'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'optimisticRepeatInstall'
| 'linkWorkspacePackages'
| 'lockfile'
| 'lockfileDir'
| 'lockfileOnly'
| 'modulesDir'
| 'nodeLinker'
| 'pnprServer'
| 'remoteSideEffectsCache'
| 'production'
| 'preferWorkspacePackages'
| 'registriesByScope'
| 'runtime'
| 'runtimeOnFail'
| 'save'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
| 'lockfileIncludeTarballUrl'
| 'scriptsPrependNodePath'
| 'scriptShell'
| 'sideEffectsCacheRead'
| 'sideEffectsCacheWrite'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'shellEmulator'
| 'tag'
| 'trustLockfile'
| 'allowBuilds'
| 'optional'
| 'workspaceConcurrency'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'extraEnv'
| 'ignoreWorkspace'
| 'ignoreWorkspaceCycles'
| 'disallowWorkspaceCycles'
| 'configDependencies'
| 'packageExtensions'
| 'updateConfig'
| 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'allProjects'
| 'allProjectsGraph'
| 'cliOptions'
| 'hooks'
| 'rootProjectManifestDir'
| 'rootProjectManifest'
| 'selectedProjectsGraph'
> & Partial<Pick<Config, 'ci' | 'loglevel' | 'reporter'>>
& CreateStoreControllerOptions & {
  argv: {
    cooked?: string[]
    original: string[]
    remain?: string[]
  }
  allowNew?: boolean
  deploy?: boolean
  /** See {@link RecursiveOptions.excludeWorkspaceRootProject}. */
  excludeWorkspaceRootProject?: boolean
  forceFullResolution?: boolean
  frozenLockfileIfExists?: boolean
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  peer?: boolean
  latest?: boolean
  /**
   * If specified, the installation will only be performed for comparison of the
   * wanted lockfile. The wanted lockfile will not be updated on disk and no
   * modules will be linked.
   *
   * The given callback is passed the wanted lockfile before installation and
   * after. This allows functions to reasonably determine whether the wanted
   * lockfile will change on disk after installation. The lockfile arguments
   * passed to this callback should not be mutated.
   */
  lockfileCheck?: (prev: LockfileObject, next: LockfileObject) => void
  update?: boolean
  updatePatches?: boolean
  updateToLatest?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest?: boolean
  useBetaCli?: boolean
  recursive?: boolean
  dedupe?: boolean
  workspace?: boolean
  interactiveUpdate?: boolean
  includeOnlyPackageFiles?: boolean
  pruneLockfileImporters?: boolean
  /**
   * Set to `false` for an install whose projects are not the workspace's own,
   * such as the legacy `pnpm deploy`. The workspace state file then keeps
   * describing the workspace's last install.
   */
  saveWorkspaceState?: boolean
  rebuildHandler?: CommandHandler
  pnpmfile: string[]
  packageVulnerabilityAudit?: PackageVulnerabilityAudit
  /**
   * `true` when this call originated from `pnpm install` (or `pnpm i`),
   * `false`/`undefined` for `add`, `update`, `dedupe`, etc. Used to gate
   * which pnpm CLI flags are safe to forward to pacquet's `install`
   * subcommand.
   */
  isInstallCommand?: boolean
} & Partial<Pick<Config, 'dangerouslyAllowAllBuilds' | 'dryRun' | 'pnpmHomeDir' | 'strictDepBuilds' | 'useLockfile' | 'useGitBranchLockfile' | 'mergeGitBranchLockfiles'>>

export async function installDeps (
  opts: InstallDepsOptions,
  params: string[]
): Promise<DryRunInstallResult | undefined> {
  if (await reportAlreadyUpToDate(opts, params)) return undefined
  if (opts.workspace) {
    applyWorkspaceOption(opts)
  }
  const store = await createStoreController(opts)
  const runPacquet = await createPacquetRunner(opts)
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  const allProjects = await readAllProjects(opts)
  if (opts.workspaceDir) {
    const selectedProjectsGraph = opts.selectedProjectsGraph ?? selectProjectByDir(allProjects, opts.dir)
    if (selectedProjectsGraph != null) {
      return installWorkspaceProjects({
        opts,
        allProjects,
        params,
        workspaceDir: opts.workspaceDir,
        selectedProjectsGraph,
        store,
        runPacquet,
      })
    }
  }
  return installSingleProject({ opts, allProjects, store, runPacquet, includeDirect }, params)
}

/**
 * The optimistic repeat-install short-circuit: returns `true` after reporting
 * "Already up to date" when nothing needs to be installed.
 */
async function reportAlreadyUpToDate (opts: InstallDepsOptions, params: string[]): Promise<boolean> {
  if (opts.update || opts.dedupe || opts.force || params.length !== 0 || !opts.optimisticRepeatInstall) return false
  if (opts.nodeLinker === 'hoisted' && (opts.hooks?.customFetchers?.length ?? 0) > 0) return false
  const { upToDate, wantedLockfileToRestore } = await checkDepsStatus({
    ...opts,
    ignoreFilteredInstallCache: true,
    treatLocalFileDepsAsOutdated: true,
  })
  if (
    !upToDate ||
    await findDanglingDirectDependencyLink(opts) != null ||
    !await restoreWantedLockfileIfMissing(wantedLockfileToRestore, opts)
  ) {
    return false
  }
  if (opts.hooks?.customResolvers?.some(resolver => resolver.shouldRefreshResolution)) {
    logger.warn({
      message: 'shouldRefreshResolution hooks were skipped because optimisticRepeatInstall is enabled.',
      prefix: opts.dir,
    })
  }
  await assertRecordedBuildsAreApproved(opts)
  globalInfo('Already up to date')
  return true
}

function applyWorkspaceOption (opts: InstallDepsOptions): void {
  if (opts.latest) {
    throw new PnpmError('BAD_OPTIONS', 'Cannot use --latest with --workspace simultaneously')
  }
  if (!opts.workspaceDir) {
    throw new PnpmError('WORKSPACE_OPTION_OUTSIDE_WORKSPACE', '--workspace can only be used inside a workspace')
  }
  if (!opts.linkWorkspacePackages && !opts.saveWorkspaceProtocol) {
    opts.saveWorkspaceProtocol = true
  }
  // @ts-expect-error -- preserveWorkspaceProtocol is an install option that InstallDepsOptions does not declare
  opts['preserveWorkspaceProtocol'] = !opts.linkWorkspacePackages
}

/**
 * When `configDependencies` declares pacquet, build the alternative
 * install engine the deps-installer delegates to. Threaded through both
 * the workspace recursive path and the single-project path.
 */
async function createPacquetRunner (opts: InstallDepsOptions): Promise<ReturnType<typeof makeRunPacquet> | undefined> {
  const pacquetConfigDepName = await findVerifiedPacquetConfigDepName(opts)
  if (pacquetConfigDepName == null || opts.deploy) return undefined
  return makeRunPacquet({
    lockfileDir: opts.lockfileDir ?? opts.dir,
    packageName: pacquetConfigDepName,
    argv: { original: opts.argv.original, remain: opts.argv.remain ?? [] },
    isInstallCommand: opts.isInstallCommand === true,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    forceIgnoresPlatform: opts.forceIgnoresPlatform !== false,
  })
}

/**
 * `configDependencies` come from the repository's `pnpm-workspace.yaml`, so
 * the declaration cannot be trusted to authorize spawning a native binary on
 * its own. `verifyPacquetIdentity` confirms, against the canonical npm
 * registry, that the installed bytes carry a valid registry signature for
 * that `name@version` before we delegate; otherwise we fall back to pnpm's
 * own engine.
 */
async function findVerifiedPacquetConfigDepName (opts: InstallDepsOptions): Promise<MakeRunPacquetOpts['packageName'] | undefined> {
  const declaredPacquetConfigDepName = findDeclaredPacquetConfigDepName(opts.configDependencies)
  if (declaredPacquetConfigDepName == null) return undefined
  const verified = await verifyPacquetIdentity(declaredPacquetConfigDepName, {
    ...opts,
    lockfileDir: opts.lockfileDir ?? opts.dir,
    rootDir: opts.lockfileDir ?? opts.dir,
  })
  return verified ? declaredPacquetConfigDepName : undefined
}

function findDeclaredPacquetConfigDepName (configDependencies: InstallDepsOptions['configDependencies']): MakeRunPacquetOpts['packageName'] | undefined {
  if (configDependencies?.['@pnpm/pacquet'] != null) return '@pnpm/pacquet'
  if (configDependencies?.pacquet != null) return 'pacquet'
  return undefined
}

async function readAllProjects (opts: InstallDepsOptions): Promise<Project[]> {
  const allProjects = opts.allProjects ?? (
    opts.workspaceDir
      ? await findWorkspaceProjects(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns })
      : []
  )
  if (opts.runtimeOnFail) {
    for (const project of allProjects) {
      applyRuntimeOnFailOverride(project.manifest, opts.runtimeOnFail)
    }
  }
  return allProjects
}

interface WorkspaceProjectsInstall {
  opts: InstallDepsOptions
  allProjects: Project[]
  params: string[]
  workspaceDir: string
  selectedProjectsGraph: ProjectsGraph
  store: Awaited<ReturnType<typeof createStoreController>>
  runPacquet: ReturnType<typeof makeRunPacquet> | undefined
}

async function installWorkspaceProjects (
  { opts, allProjects, params, workspaceDir, selectedProjectsGraph, store, runPacquet }: WorkspaceProjectsInstall
): Promise<DryRunInstallResult | undefined> {
  failOrWarnOnWorkspaceCycles(opts, { workspaceDir, selectedProjectsGraph })
  await warnAboutNestedWorkspaceManifests(workspaceDir, Object.keys(selectedProjectsGraph))

  const allProjectsGraph: ProjectsGraph = opts.allProjectsGraph ?? createProjectsGraph(allProjects, {
    catalogs: opts.catalogs,
    linkWorkspacePackages: Boolean(opts.linkWorkspacePackages),
  }).graph

  return recursiveInstallThenUpdateWorkspaceState(allProjects,
    params,
    {
      ...opts,
      preferredVersions: opts.packageVulnerabilityAudit ? preferNonvulnerablePackageVersions(opts.packageVulnerabilityAudit) : undefined,
      allProjectsGraph,
      selectedProjectsGraph,
      storeControllerAndDir: store,
      workspaceDir,
      runPacquet,
    },
    opts.update ? 'update' : (params.length === 0 ? 'install' : 'add')
  )
}

function failOrWarnOnWorkspaceCycles (
  opts: Pick<InstallDepsOptions, 'ignoreWorkspaceCycles' | 'disallowWorkspaceCycles'>,
  { workspaceDir, selectedProjectsGraph }: { workspaceDir: string, selectedProjectsGraph: ProjectsGraph }
): void {
  const sequencedGraph = sequenceGraph(selectedProjectsGraph)
  if (opts.ignoreWorkspaceCycles || !sequencedGraph.cycles.some((cycle) => cycle.length > 1)) return
  const cyclicDependenciesInfo = sequencedGraph.cycles.length > 0
    ? `: ${sequencedGraph.cycles.map(deps => deps.join(', ')).join('; ')}`
    : ''

  if (opts.disallowWorkspaceCycles) {
    throw new PnpmError('DISALLOW_WORKSPACE_CYCLES', `There are cyclic workspace dependencies${cyclicDependenciesInfo}`)
  }

  logger.warn({
    message: `There are cyclic workspace dependencies${cyclicDependenciesInfo}`,
    prefix: workspaceDir,
  })
}

function selectProjectByDir (projects: Project[], searchedDir: string): ProjectsGraph | undefined {
  const project = projects.find(({ rootDir }) => path.relative(rootDir, searchedDir) === '')
  if (project == null) return undefined
  return { [project.rootDir]: { dependencies: [], package: project } }
}

/**
 * Restore a missing `pnpm-lock.yaml` from the current lockfile before the
 * optimistic repeat-install short-circuit reports "Already up to date", so
 * the fast path leaves the same on-disk contract a full install would.
 * Returns `true` when the short-circuit may proceed: nothing to restore,
 * lockfile writing is disabled (`useLockfile: false`), or the restore
 * succeeded. A failed write returns `false` so the caller falls through to
 * the full install instead of reporting up to date while `pnpm-lock.yaml`
 * stays missing.
 */
async function restoreWantedLockfileIfMissing (
  wantedLockfileToRestore: { lockfile: LockfileObject, lockfileDir: string } | undefined,
  opts: Pick<InstallDepsOptions, 'useLockfile'>
): Promise<boolean> {
  if (wantedLockfileToRestore == null || opts.useLockfile === false) return true
  try {
    await writeWantedLockfile(wantedLockfileToRestore.lockfileDir, wantedLockfileToRestore.lockfile)
    return true
  } catch (error) {
    logger.debug({ msg: 'Failed to restore pnpm-lock.yaml from the current lockfile', error })
    return false
  }
}

/**
 * The optimistic repeat-install short-circuit returns before the build policy
 * runs, so a package left with an undecided build would never be reported and
 * `strictDepBuilds` would go unenforced until `node_modules` was cleared.
 * Fail here the way a materializing install would.
 */
async function assertRecordedBuildsAreApproved (opts: InstallDepsOptions): Promise<void> {
  if (opts.ignoreScripts || opts.lockfileOnly || !opts.strictDepBuilds) return
  const modulesDir = path.resolve(opts.lockfileDir ?? opts.dir, opts.modulesDir ?? 'node_modules')
  const modulesManifest = await readModulesManifest(modulesDir)
  const unapprovedBuilds = unapprovedIgnoredBuilds(
    modulesManifest?.ignoredBuilds,
    createAllowBuildFunction(opts)
  )
  if (unapprovedBuilds.length) throw new IgnoredBuildsError(new Set(unapprovedBuilds))
}
