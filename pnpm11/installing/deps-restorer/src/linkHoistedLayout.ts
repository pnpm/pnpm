import { promises as fs } from 'node:fs'
import path from 'node:path'

import { linkBinsOfPackages } from '@pnpm/bins.linker'
import { stageLogger } from '@pnpm/core-loggers'
import type { DependenciesGraph, DepHierarchy } from '@pnpm/deps.graph-builder'
import { isError } from '@pnpm/error'
import { hoistWorkspacePackages, pruneStaleWorkspaceHoists } from '@pnpm/installing.linking.hoist'
import type { IncludedDependencies } from '@pnpm/installing.modules-yaml'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DependencyManifest, DepPath, HoistedDependencies, ProjectId } from '@pnpm/types'

import type { HeadlessContext, HeadlessDepGraph, LinkedDependencies } from './context.js'
import { linkHoistedModules } from './linkHoistedModules.js'
import { reportDirectDependencyChanges } from './reportDirectDependencyChanges.js'
import { symlinkDirectDependencies } from './symlinkDirectDependencies.js'
import { getHoistedWorkspacePackages, removeBinsOfWorkspaceHoists } from './workspaceHoists.js'

export interface HoistedLayout {
  hierarchy: DepHierarchy
  prevGraph: DependenciesGraph
}

export async function linkHoistedLayout (ctx: HeadlessContext, depGraph: HeadlessDepGraph, layout: HoistedLayout): Promise<LinkedDependencies> {
  const { opts } = ctx
  if (ctx.skipPostImportLinking) {
    return { heldBackBinsDirs: [], linkedToRoot: 0, newHoistedDependencies: opts.hoistedDependencies }
  }
  const hoistsWorkspacePackages = opts.ignorePackageManifest !== true
  if (hoistsWorkspacePackages) {
    // Unlinked before the linker writes the tree: a package that takes over
    // a project's name keeps the modules directory of what it replaces, which
    // through the symlink is the project's own node_modules.
    await unlinkPriorWorkspaceHoists(ctx)
  }
  const heldBackBinsDirs = await linkHoistedTree(ctx, depGraph, layout)
  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'importing_done',
  })
  const newHoistedDependencies = hoistsWorkspacePackages
    ? await hoistWorkspacePackagesToRoot(ctx, depGraph)
    : opts.hoistedDependencies
  const linkedToRoot = await symlinkDirectDependencies({
    directDependenciesByImporterId: depGraph.symlinkedDirectDependenciesByImporterId!,
    dedupe: Boolean(opts.dedupeDirectDeps),
    filteredLockfile: depGraph.filteredLockfile,
    lockfileDir: ctx.lockfileDir,
    projects: ctx.selectedProjects,
    registriesByScope: opts.registriesByScope,
    symlink: opts.symlink,
  })
  reportDirectDependencyChanges({
    currentLockfile: ctx.currentLockfile,
    wantedLockfile: depGraph.filteredLockfile,
    projects: ctx.selectedProjects,
    previouslySkipped: new Set(opts.modulesFile?.skipped as DepPath[] | undefined),
    skipped: ctx.skipped,
  })
  return { heldBackBinsDirs, linkedToRoot, newHoistedDependencies }
}

async function unlinkPriorWorkspaceHoists ({ opts, currentLockfile, wantedLockfile, rootModulesDir }: HeadlessContext): Promise<void> {
  const priorWorkspaceProjectIds = new Set(Object.keys(opts.hoistedDependencies)
    .filter((key) => currentLockfile?.packages?.[key as DepPath] == null && wantedLockfile.packages?.[key as DepPath] == null) as ProjectId[])
  await removeBinsOfWorkspaceHoists(opts.hoistedDependencies, priorWorkspaceProjectIds, rootModulesDir)
  await pruneStaleWorkspaceHoists(opts.hoistedDependencies, {}, priorWorkspaceProjectIds, rootModulesDir, rootModulesDir)
}

async function linkHoistedTree (ctx: HeadlessContext, depGraph: HeadlessDepGraph, { hierarchy, prevGraph }: HoistedLayout): Promise<string[]> {
  const { opts } = ctx
  return linkHoistedModules(opts.storeController, depGraph.graph, prevGraph, hierarchy, {
    allowBuild: depGraph.allowBuild,
    deferDependencyBuilds: opts.deferDependencyBuilds === true,
    depsStateCache: ctx.depsStateCache,
    disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
    force: opts.force,
    holdBackMissingBins: !opts.ignoreScripts && opts.enableModulesDir !== false && !opts.ignorePackageManifest,
    ignoreScripts: opts.ignoreScripts,
    lockfileDir: opts.lockfileDir,
    nodeVersion: ctx.rootRuntimeNodeVersion,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    sideEffectsCacheRead: opts.sideEffectsCacheRead,
    remoteSideEffectsCache: opts.remoteSideEffectsCache,
    pnprServer: opts.pnprServer,
    configByUri: opts.configByUri,
    supportedArchitectures: opts.supportedArchitectures,
  })
}

async function hoistWorkspacePackagesToRoot (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<HoistedDependencies> {
  const { opts, rootModulesDir } = ctx
  // Every package of the hoisted layout lives in the root modules
  // directory, so a project matching either hoist pattern is linked there.
  const hoistedDependencies = await hoistWorkspacePackages({
    directDepsByImporterId: {
      '.': new Map(Object.entries(depGraph.directDependenciesByImporterId['.'] ?? {})),
    },
    graph: depGraph.graph,
    // The root's linked dependencies take their names after this pass.
    hoistedWorkspacePackages: opts.hoistWorkspacePackages
      ? getHoistedWorkspacePackages(opts.allProjects, getRootDependencyAliases(depGraph.filteredLockfile, opts.include))
      : undefined,
    privateHoistedModulesDir: rootModulesDir,
    privateHoistPattern: opts.hoistPattern ?? [],
    publicHoistedModulesDir: rootModulesDir,
    publicHoistPattern: opts.publicHoistPattern ?? [],
    virtualStoreDir: ctx.virtualStoreDir,
  })
  await linkBinsOfPackages(
    Object.values(opts.allProjects)
      .filter((project) => hoistedDependencies[project.id] != null)
      .map((project) => ({ location: project.rootDir, manifest: project.manifest as DependencyManifest })),
    path.join(rootModulesDir, '.bin'),
    {
      excludeBins: await readCommandNames(path.join(rootModulesDir, '.bin')),
      extraNodePaths: opts.extraNodePaths,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    }
  )
  return hoistedDependencies
}

function getRootDependencyAliases (lockfile: LockfileObject, include: IncludedDependencies): string[] {
  const root = lockfile.importers['.' as ProjectId]
  if (root == null) return []
  return Object.keys({
    ...(include.dependencies ? root.dependencies : {}),
    ...(include.devDependencies ? root.devDependencies : {}),
    ...(include.dependencies && include.optionalDependencies ? root.optionalDependencies : {}),
  })
}

const WINDOWS_BIN_EXTENSIONS = new Set(['.cmd', '.ps1', '.exe'])

/**
 * The commands already linked into `binsDir`, so that a workspace project's
 * bins never replace the bins of a hoisted package. On Windows the shim and
 * executable extensions are stripped in any case. A missing `binsDir` has no
 * commands.
 */
async function readCommandNames (binsDir: string): Promise<Set<string>> {
  let entries: string[]
  try {
    entries = await fs.readdir(binsDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return new Set()
    throw err
  }
  if (process.platform !== 'win32') return new Set(entries)
  return new Set(entries.map((entry) => {
    const extension = path.extname(entry)
    return WINDOWS_BIN_EXTENSIONS.has(extension.toLowerCase()) ? entry.slice(0, -extension.length) : entry
  }))
}
