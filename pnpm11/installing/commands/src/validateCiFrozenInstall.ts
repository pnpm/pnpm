import { binDirOf } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { validateFrozenInstall, verifyPatchesOfLockfiles } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { createStoreController } from '@pnpm/store.connection-manager'
import type { IncludedDependencies, Project, ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'

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

/**
 * Validates that `pnpm ci` can complete its `--frozen-lockfile` install before
 * any `node_modules` directory is removed.
 */
export async function validateCiFrozenInstall (opts: InstallCommandOptions): Promise<void> {
  if (opts.global) {
    throw new PnpmError('GLOBAL_INSTALL_NOT_SUPPORTED',
      '"pnpm install -g" is not supported. Use "pnpm add -g <pkg>" to install global packages.')
  }
  if (opts.dryRun || opts.lockfileOnly || opts.resolutionOnly || opts.fixLockfile) return
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
  try {
    await validateWithStore(installDepsOptions, store, include)
  } finally {
    await store.ctrl.close()
  }
}

async function validateWithStore (
  installDepsOptions: InstallDepsOptions,
  store: StoreControllerAndDir,
  include: IncludedDependencies
): Promise<void> {
  const allProjects = await readAllProjects(installDepsOptions)
  if (installDepsOptions.workspaceDir) {
    const selectedProjectsGraph = installDepsOptions.selectedProjectsGraph ??
      selectProjectByDir(allProjects, installDepsOptions.dir)
    if (selectedProjectsGraph != null) {
      await validateWorkspaceCi({
        allProjects,
        opts: installDepsOptions,
        selectedProjectsGraph,
        store,
        workspaceDir: installDepsOptions.workspaceDir,
      })
      return
    }
  }
  await validateSingleProjectCi(installDepsOptions, allProjects, store, include)
}

async function validateSingleProjectCi (
  opts: InstallDepsOptions,
  allProjects: Project[],
  store: StoreControllerAndDir,
  includeDirect: IncludedDependencies
): Promise<void> {
  const ctx = await createSingleProjectInstallContext(
    { opts, allProjects, store, runPacquet: undefined, includeDirect },
    false
  )
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir
  await validateFrozenInstall(
    [{ mutation: 'install', pruneDirectDependencies: opts.pruneDirectDependencies, rootDir }],
    {
      ...ctx.installOpts,
      allProjects: [{
        buildIndex: 0,
        manifest: ctx.manifest,
        rootDir,
        binsDir: opts.bin,
      }],
    }
  )
}

async function validateWorkspaceCi (
  { allProjects, opts, selectedProjectsGraph, store, workspaceDir }: {
    allProjects: Project[]
    opts: InstallDepsOptions
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
    await validateSharedLockfileWorkspace(ctx)
    return
  }
  await validateDedicatedLockfilesWorkspace(ctx)
}

async function validateSharedLockfileWorkspace (ctx: RecursiveContext): Promise<void> {
  const mutatedImporters = await prepareSharedLockfileImporters(ctx)
  if (mutatedImporters == null) return
  await validateFrozenInstall(mutatedImporters, {
    ...ctx.installOpts,
    storeController: ctx.store.ctrl,
    resolutionVerifiers: ctx.store.resolutionVerifiers,
  })
}

async function validateDedicatedLockfilesWorkspace (ctx: RecursiveContext): Promise<void> {
  const pkgPaths = (Object.keys(ctx.opts.selectedProjectsGraph) as ProjectRootDir[]).sort()
  const validatedDirs = new Set<ProjectRootDir>()
  const lockfiles: LockfileObject[] = []
  for (const rootDir of pkgPaths) {
    if (ctx.opts.ignoredPackages?.has(rootDir)) continue
    // eslint-disable-next-line no-await-in-loop -- fail fast on the first invalid project lockfile
    const lockfile = await validateSingleDedicatedProject(ctx, rootDir)
    validatedDirs.add(rootDir)
    if (lockfile != null) {
      lockfiles.push(lockfile)
    }
  }
  if (ctx.installOpts.useLockfile === false || ctx.opts.lockfile === false) return
  if (!ctx.allProjects.every(({ rootDir }) => validatedDirs.has(rootDir))) return
  await verifyPatchesOfLockfiles(lockfiles, {
    patchedDependencies: ctx.installOpts.patchedDependencies,
    lockfileDir: ctx.opts.workspaceDir,
    allowUnusedPatches: ctx.installOpts.allowUnusedPatches,
  })
}

async function validateSingleDedicatedProject (
  ctx: RecursiveContext,
  rootDir: ProjectRootDir
): Promise<LockfileObject | undefined> {
  const hooks = await loadProjectHooks(ctx.opts, rootDir)
  const { manifest } = ctx.manifestsByPath[rootDir]
  const localConfig = ctx.getProjectConfig(manifest) ?? {}
  const binsDir = binDirOf(rootDir, localConfig.modulesDir ?? ctx.opts.modulesDir)
  return validateFrozenInstall(
    [{ mutation: 'install', rootDir }],
    {
      ...ctx.installOpts,
      ...localConfig,
      ...ctx.opts.allProjectsGraph[rootDir]?.package,
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
}
