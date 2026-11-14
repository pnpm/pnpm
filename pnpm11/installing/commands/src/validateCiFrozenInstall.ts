import { promises as fs } from 'node:fs'
import path from 'node:path'

import { binDirOf, getWorkspaceConcurrency } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
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
import { isSubdir, strict as isStrictSubdir } from 'is-subdir'

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

export type CleansModulesDirFn = (anchorDir: string, modulesDir?: string) => Promise<boolean>
type CiInstallDepsOptions = InstallDepsOptions & { _cleansModulesDir?: CleansModulesDirFn }

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
export async function validateCiFrozenInstall (
  opts: InstallCommandOptions & { _cleansModulesDir?: CleansModulesDirFn }
): Promise<PreparedCiInstall | undefined> {
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
  const installDepsOptions: CiInstallDepsOptions = {
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
    return await validateWithStore({ close, include, installDepsOptions, store })
  } catch (err) {
    await close()
    throw err
  }
}

interface ValidateWithStoreOptions {
  close: () => Promise<void>
  include: IncludedDependencies
  installDepsOptions: CiInstallDepsOptions
  store: StoreControllerAndDir
}

async function validateWithStore (
  { close, include, installDepsOptions, store }: ValidateWithStoreOptions
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
    opts: CiInstallDepsOptions
    preparedCi: PreparedCiInstall
    store: StoreControllerAndDir
  }
): Promise<void> {
  const ctx = await createSingleProjectInstallContext(
    { opts, allProjects, store, runPacquet: undefined, includeDirect },
    false
  )
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir
  const anchorDir = opts.lockfileDir ?? rootDir
  const cleansVirtualStore = await (opts._cleansModulesDir?.(anchorDir, opts.modulesDir) ?? cleansAnchoredModulesDir({
    anchorDir,
    cleanupRootDir: rootDir,
    modulesDir: opts.modulesDir,
  }))
  const validated = await validateFrozenInstall(
    [{ mutation: 'install', pruneDirectDependencies: opts.pruneDirectDependencies, rootDir }],
    {
      ...ctx.installOpts,
      _cleansVirtualStore: cleansVirtualStore,
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
    opts: CiInstallDepsOptions
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
  await validateDedicatedLockfilesWorkspace(ctx, opts, preparedCi)
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
  opts: CiInstallDepsOptions,
  preparedCi: PreparedCiInstall
): Promise<void> {
  const mutatedImporters = await prepareSharedLockfileImporters(ctx)
  if (mutatedImporters == null) return
  const cleansVirtualStore = !isNarrowedWorkspaceCi(opts) &&
    (ctx.opts.lockfileDir == null || await (opts._cleansModulesDir?.(ctx.opts.lockfileDir, opts.modulesDir) ?? cleansAnchoredModulesDir({
      allProjects: ctx.allProjects,
      anchorDir: ctx.opts.lockfileDir,
      cleanupRootDir: ctx.opts.workspaceDir,
      modulesDir: opts.modulesDir,
    })))
  const validated = await validateFrozenInstall(mutatedImporters, {
    ...ctx.installOpts,
    _cleansVirtualStore: cleansVirtualStore,
    storeController: ctx.store.ctrl,
    resolutionVerifiers: ctx.store.resolutionVerifiers,
  })
  if (validated?.prepared != null && ctx.opts.lockfileDir != null) {
    preparedCi.preparedByLockfileDir.set(ctx.opts.lockfileDir, validated.prepared)
  }
}

async function validateDedicatedLockfilesWorkspace (
  ctx: RecursiveContext,
  opts: CiInstallDepsOptions,
  preparedCi: PreparedCiInstall
): Promise<void> {
  const pkgPaths = (Object.keys(ctx.opts.selectedProjectsGraph) as ProjectRootDir[]).sort()
  const validatedDirs = new Set<ProjectRootDir>()
  const lockfilesByDir = new Map<ProjectRootDir, LockfileObject>()
  const dedicatedEnv: DedicatedValidationEnv = { cleansModulesDir: opts._cleansModulesDir, narrowed: isNarrowedWorkspaceCi(opts), preparedCi }
  let firstError: unknown
  await scheduleGraph(new Map(pkgPaths.map((rootDir) => [rootDir, []])), {
    bail: true,
    concurrency: getWorkspaceConcurrency(ctx.opts.workspaceConcurrency),
    continueOnFailure: false,
    onNodeSkipped: () => {},
    runNode: async (rootDir): Promise<TaskCompletion> => {
      if (ctx.opts.ignoredPackages?.has(rootDir)) return 'passed'
      try {
        const lockfile = await validateSingleDedicatedProject(ctx, rootDir, dedicatedEnv)
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
  await verifyDedicatedWorkspacePatches(ctx, { lockfilesByDir, pkgPaths, validatedDirs })
}

interface DedicatedWorkspacePatchesInput {
  lockfilesByDir: Map<ProjectRootDir, LockfileObject>
  pkgPaths: ProjectRootDir[]
  validatedDirs: Set<ProjectRootDir>
}

async function verifyDedicatedWorkspacePatches (
  ctx: RecursiveContext,
  { lockfilesByDir, pkgPaths, validatedDirs }: DedicatedWorkspacePatchesInput
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

interface DedicatedValidationEnv {
  cleansModulesDir?: CleansModulesDirFn
  narrowed: boolean
  preparedCi: PreparedCiInstall
}

async function validateSingleDedicatedProject (
  ctx: RecursiveContext,
  rootDir: ProjectRootDir,
  { cleansModulesDir, narrowed, preparedCi }: DedicatedValidationEnv
): Promise<LockfileObject | undefined> {
  const hooks = await loadProjectHooks(ctx.opts, rootDir)
  preparedCi.projectHooksByRootDir.set(rootDir, hooks)
  const { manifest } = ctx.manifestsByPath[rootDir]
  const localConfig = ctx.getProjectConfig(manifest) ?? {}
  const modulesDir = localConfig.modulesDir ?? ctx.opts.modulesDir
  const cleansVirtualStore = await cleansDedicatedProjectModules(ctx, rootDir, { cleansModulesDir, modulesDir, narrowed })
  const validated = await validateFrozenInstall(
    [{ mutation: 'install', rootDir }],
    {
      ...ctx.installOpts,
      ...localConfig,
      ...ctx.opts.allProjectsGraph[rootDir]?.package,
      _cleansVirtualStore: cleansVirtualStore,
      dir: rootDir,
      lockfileDir: rootDir,
      hooks,
      ignoreScripts: true,
      storeController: ctx.store.ctrl,
      resolutionVerifiers: ctx.store.resolutionVerifiers,
      allProjects: [{ buildIndex: 0, manifest, rootDir, binsDir: binDirOf(rootDir, modulesDir) }],
    }
  )
  if (validated?.prepared != null) {
    preparedCi.preparedByLockfileDir.set(rootDir, validated.prepared)
  }
  return validated?.wantedLockfile
}

async function cleansDedicatedProjectModules (
  ctx: RecursiveContext,
  rootDir: ProjectRootDir,
  { cleansModulesDir, modulesDir, narrowed }: { cleansModulesDir?: CleansModulesDirFn, modulesDir?: string, narrowed: boolean }
): Promise<boolean> {
  if (cleansModulesDir != null) return cleansModulesDir(rootDir, modulesDir)
  const allowRootModulesTarget = !narrowed &&
    path.resolve(ctx.opts.workspaceDir, ctx.opts.modulesDir ?? 'node_modules') ===
    path.resolve(rootDir, modulesDir ?? 'node_modules')
  return cleansAnchoredModulesDir({
    allProjects: ctx.allProjects,
    allowRootModulesTarget,
    anchorDir: rootDir,
    cleanupRootDir: ctx.opts.workspaceDir,
    modulesDir,
    projectDir: rootDir,
  })
}

async function cleansAnchoredModulesDir (
  { allProjects, allowRootModulesTarget = false, anchorDir, cleanupRootDir, modulesDir = 'node_modules', projectDir }: {
    allProjects?: Project[]
    allowRootModulesTarget?: boolean
    anchorDir: string
    cleanupRootDir: string
    modulesDir?: string
    projectDir?: string
  }
): Promise<boolean> {
  const candidate = path.resolve(anchorDir, modulesDir)
  const [realCleanupRoot, realParent, realProjectDir] = await Promise.all([
    realpathIfExists(cleanupRootDir),
    realpathIfExists(path.dirname(candidate)),
    projectDir != null ? realpathIfExists(projectDir) : Promise.resolve(undefined),
  ])
  if (realCleanupRoot == null || realParent == null) return false
  const resolved = path.join(realParent, path.basename(candidate))
  if (!isStrictSubdir(realCleanupRoot, resolved)) return false
  if (!allowRootModulesTarget && realProjectDir != null && !isStrictSubdir(realProjectDir, resolved)) {
    return false
  }
  return !(await overlapsProtectedMember(resolved, realCleanupRoot, allProjects))
}

async function overlapsProtectedMember (
  resolved: string,
  realCleanupRoot: string,
  allProjects: Project[] | undefined
): Promise<boolean> {
  if (!allProjects?.length) return false
  const realMemberDirs = await Promise.all(allProjects.map(async ({ rootDir }) => realpathIfExists(rootDir)))
  return realMemberDirs.some((dir) => dir != null && dir !== realCleanupRoot && isSubdir(resolved, dir))
}

async function realpathIfExists (targetPath: string): Promise<string | undefined> {
  try {
    return await fs.realpath(targetPath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR' || err.code === 'ELOOP')) {
      return undefined
    }
    throw err
  }
}
