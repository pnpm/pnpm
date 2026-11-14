import { binDirOf, getWorkspaceConcurrency } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import {
  type InstallOptions,
  type PreparedFrozenMutation,
  validateFrozenInstall,
  verifyPatchesOfLockfiles,
} from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { createStoreController } from '@pnpm/store.connection-manager'
import type { IncludedDependencies, Project, ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'

import type { InstallCommandOptions } from './install.js'
import {
  applyWorkspaceOption,
  failOrWarnOnWorkspaceCycles,
  type InstallDepsOptions,
  readAllProjects,
  selectProjectByDir,
} from './installDeps.js'
import { createSingleProjectInstallContext } from './installSingleProject.js'
import { createRecursiveContext, type RecursiveContext } from './recursive/context.js'
import { loadProjectHooks } from './recursive/installEachProject.js'
import { prepareSharedLockfileImporters } from './recursive/installWithSharedLockfile.js'
import { loadLockedRegistriesByScope } from './workspaceStateUpdate.js'

type StoreControllerAndDir = Awaited<ReturnType<typeof createStoreController>>

export interface PreparedCiInstall {
  allProjects: Project[]
  close: () => Promise<void>
  preparedByLockfileDir: Map<string, PreparedFrozenMutation>
  projectHooksByRootDir: Map<ProjectRootDir, InstallOptions['hooks']>
  store: StoreControllerAndDir
}

/**
 * Validates that `pnpm ci` can complete its `--frozen-lockfile` install before
 * any `node_modules` directory is removed, returning reusable store and hook
 * state for the post-clean install.
 */
export async function validateCiFrozenInstall (opts: InstallCommandOptions): Promise<PreparedCiInstall | undefined> {
  if (opts.global) {
    throw new PnpmError('GLOBAL_INSTALL_NOT_SUPPORTED',
      '"pnpm install -g" is not supported. Use "pnpm add -g <pkg>" to install global packages.')
  }
  if (opts.dryRun || opts.lockfileOnly || opts.resolutionOnly) return undefined
  const include: IncludedDependencies = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const installDepsOptions: InstallDepsOptions = {
    ...opts,
    frozenLockfile: true,
    frozenLockfileIfExists: false,
    include,
    includeDirect: include,
    isInstallCommand: true,
    lockedRegistriesByScope: loadLockedRegistriesByScope(opts as InstallDepsOptions),
  }
  if (installDepsOptions.workspace) {
    applyWorkspaceOption(installDepsOptions)
  }
  const store = await createStoreController({ ...installDepsOptions })
  let closed = false
  const close = async (): Promise<void> => {
    if (closed) return
    closed = true
    await store.ctrl.close()
  }
  try {
    return await validateWithStore(installDepsOptions, store, include, close)
  } catch (err) {
    await close()
    throw err
  }
}

async function validateWithStore (
  installDepsOptions: InstallDepsOptions,
  store: StoreControllerAndDir,
  include: IncludedDependencies,
  close: () => Promise<void>
): Promise<PreparedCiInstall> {
  const allProjects = await readAllProjects(installDepsOptions)
  const preparedCi: PreparedCiInstall = {
    allProjects,
    close,
    preparedByLockfileDir: new Map<string, PreparedFrozenMutation>(),
    projectHooksByRootDir: new Map<ProjectRootDir, InstallOptions['hooks']>(),
    store,
  }
  if (installDepsOptions.workspaceDir) {
    const selectedProjectsGraph = installDepsOptions.selectedProjectsGraph ??
      selectProjectByDir(allProjects, installDepsOptions.dir)
    if (selectedProjectsGraph != null) {
      await validateWorkspaceCi({
        allProjects,
        opts: installDepsOptions,
        preparedCi,
        selectedProjectsGraph,
        store,
        workspaceDir: installDepsOptions.workspaceDir,
      })
      return preparedCi
    }
  }
  await validateSingleProjectCi({
    allProjects,
    includeDirect: include,
    opts: installDepsOptions,
    preparedCi,
    store,
  })
  return preparedCi
}

async function validateSingleProjectCi (
  { allProjects, includeDirect, opts, preparedCi, store }: {
    allProjects: Project[]
    includeDirect: IncludedDependencies
    opts: InstallDepsOptions
    preparedCi: PreparedCiInstall
    store: StoreControllerAndDir
  }
): Promise<void> {
  const ctx = await createSingleProjectInstallContext(
    { opts, allProjects, store, runPacquet: undefined, includeDirect },
    false
  )
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir
  const validated = await validateFrozenInstall(
    [{ mutation: 'install', pruneDirectDependencies: opts.pruneDirectDependencies, rootDir }],
    {
      ...ctx.installOpts,
      _cleansVirtualStore: true,
      allProjects: [{
        buildIndex: 0,
        manifest: ctx.manifest,
        rootDir,
        binsDir: opts.bin,
      }],
    }
  )
  if (validated?.prepared != null) {
    preparedCi.preparedByLockfileDir.set(opts.lockfileDir ?? rootDir, validated.prepared)
  }
}

async function validateWorkspaceCi (
  { allProjects, opts, preparedCi, selectedProjectsGraph, store, workspaceDir }: {
    allProjects: Project[]
    opts: InstallDepsOptions
    preparedCi: PreparedCiInstall
    selectedProjectsGraph: ProjectsGraph
    store: StoreControllerAndDir
    workspaceDir: string
  }
): Promise<void> {
  failOrWarnOnWorkspaceCycles(opts, { workspaceDir, selectedProjectsGraph })
  if (allProjects.length === 0) return
  const selectedProjects = Object.values(selectedProjectsGraph).map((wsPkg) => wsPkg.package)
  if (selectedProjects.length === 0) return
  const allProjectsGraph: ProjectsGraph = opts.allProjectsGraph ?? createProjectsGraph(allProjects, {
    catalogs: opts.catalogs,
    linkWorkspacePackages: Boolean(opts.linkWorkspacePackages),
  }).graph
  const ctx = await createRecursiveContext({
    allProjects,
    selectedProjects,
    params: [],
    opts: {
      ...opts,
      allProjectsGraph,
      selectedProjectsGraph,
      storeControllerAndDir: store,
      workspaceDir,
    },
    cmdFullName: 'install',
  })
  if (ctx.opts.lockfileDir) {
    await validateSharedLockfileWorkspace(ctx, opts, preparedCi)
    return
  }
  await validateDedicatedLockfilesWorkspace(ctx, preparedCi)
}

function isNarrowedWorkspaceCi (opts: InstallDepsOptions): boolean {
  return Boolean(
    (opts as { filter?: string[] }).filter?.length ||
    (opts as { filterProd?: string[] }).filterProd?.length ||
    (opts as { workspaceRoot?: boolean }).workspaceRoot ||
    opts.cliOptions?.['recursive'] === false
  )
}

async function validateSharedLockfileWorkspace (
  ctx: RecursiveContext,
  opts: InstallDepsOptions,
  preparedCi: PreparedCiInstall
): Promise<void> {
  const mutatedImporters = await prepareSharedLockfileImporters(ctx)
  if (mutatedImporters == null) return
  const validated = await validateFrozenInstall(mutatedImporters, {
    ...ctx.installOpts,
    _cleansVirtualStore: !isNarrowedWorkspaceCi(opts),
    storeController: ctx.store.ctrl,
    resolutionVerifiers: ctx.store.resolutionVerifiers,
  })
  if (validated?.prepared != null && ctx.opts.lockfileDir != null) {
    preparedCi.preparedByLockfileDir.set(ctx.opts.lockfileDir, validated.prepared)
  }
}

async function validateDedicatedLockfilesWorkspace (
  ctx: RecursiveContext,
  preparedCi: PreparedCiInstall
): Promise<void> {
  const pkgPaths = (Object.keys(ctx.opts.selectedProjectsGraph) as ProjectRootDir[]).sort()
  const validatedDirs = new Set<ProjectRootDir>()
  const lockfilesByDir = new Map<ProjectRootDir, LockfileObject>()
  let firstError: unknown
  await scheduleGraph(new Map(pkgPaths.map((rootDir) => [rootDir, []])), {
    bail: true,
    concurrency: getWorkspaceConcurrency(ctx.opts.workspaceConcurrency),
    continueOnFailure: false,
    onNodeSkipped: () => {},
    runNode: async (rootDir): Promise<TaskCompletion> => {
      if (ctx.opts.ignoredPackages?.has(rootDir)) return 'passed'
      try {
        const lockfile = await validateSingleDedicatedProject(ctx, rootDir, preparedCi)
        validatedDirs.add(rootDir)
        if (lockfile != null) lockfilesByDir.set(rootDir, lockfile)
        return 'passed'
      } catch (err) {
        firstError ??= err
        return 'aborted'
      }
    },
  })
  if (firstError != null) throw firstError
  await verifyDedicatedWorkspacePatches(ctx, pkgPaths, validatedDirs, lockfilesByDir)
}

async function verifyDedicatedWorkspacePatches (
  ctx: RecursiveContext,
  pkgPaths: ProjectRootDir[],
  validatedDirs: Set<ProjectRootDir>,
  lockfilesByDir: Map<ProjectRootDir, LockfileObject>
): Promise<void> {
  if (ctx.installOpts.useLockfile === false || ctx.opts.lockfile === false) return
  if (!ctx.allProjects.every(({ rootDir }) => validatedDirs.has(rootDir))) return
  const orderedLockfiles = pkgPaths.flatMap((dir) => {
    const lockfile = lockfilesByDir.get(dir)
    return lockfile == null ? [] : [lockfile]
  })
  await verifyPatchesOfLockfiles(orderedLockfiles, {
    patchedDependencies: ctx.installOpts.patchedDependencies,
    lockfileDir: ctx.opts.workspaceDir,
    allowUnusedPatches: ctx.installOpts.allowUnusedPatches,
  })
}

async function validateSingleDedicatedProject (
  ctx: RecursiveContext,
  rootDir: ProjectRootDir,
  preparedCi: PreparedCiInstall
): Promise<LockfileObject | undefined> {
  const hooks = await loadProjectHooks(ctx.opts, rootDir)
  preparedCi.projectHooksByRootDir.set(rootDir, hooks)
  const { manifest } = ctx.manifestsByPath[rootDir]
  const localConfig = ctx.getProjectConfig(manifest) ?? {}
  const binsDir = binDirOf(rootDir, localConfig.modulesDir ?? ctx.opts.modulesDir)
  const validated = await validateFrozenInstall(
    [{ mutation: 'install', rootDir }],
    {
      ...ctx.installOpts,
      ...localConfig,
      ...ctx.opts.allProjectsGraph[rootDir]?.package,
      _cleansVirtualStore: true,
      dir: rootDir,
      lockfileDir: rootDir,
      hooks,
      ignoreScripts: true,
      storeController: ctx.store.ctrl,
      resolutionVerifiers: ctx.store.resolutionVerifiers,
      allProjects: [{
        buildIndex: 0,
        manifest,
        rootDir,
        binsDir,
      }],
    }
  )
  if (validated?.prepared != null) {
    preparedCi.preparedByLockfileDir.set(rootDir, validated.prepared)
  }
  return validated?.wantedLockfile
}
