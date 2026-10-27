import path from 'node:path'

import { getProjectNodePath, type LinkBinOptions, linkBins, linkBinsOfPackages, pruneStaleBins } from '@pnpm/bins.linker'
import { linkBinsOfRuntimeDependencies } from '@pnpm/building.during-install'
import type { DependenciesGraph, DirectDependenciesByImporterId } from '@pnpm/deps.graph-builder'
import * as dp from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import type {
  DependencyManifest,
  DepPath,
  HoistedDependencies,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
} from '@pnpm/types'
import { safeReadPublishManifest } from '@pnpm/workspace.project-manifest-reader'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'
import { limitLinking } from './limits.js'
import type { HeadlessOptions, Project } from './types.js'

export interface ImporterBuiltBinContext {
  builtDepPaths?: ReadonlySet<DepPath>
  builtPackageDirs?: ReadonlySet<string>
  newHoistedDependencies?: HoistedDependencies
}

interface ProjectBinsContext {
  built?: ImporterBuiltBinContext
  depGraph: HeadlessDepGraph
  opts: HeadlessOptions
  rootProjectDeps: Record<string, string>
}

export async function linkBinsOfImporters (
  { opts, selectedProjects }: HeadlessContext,
  depGraph: HeadlessDepGraph,
  built?: ImporterBuiltBinContext
): Promise<void> {
  const rootProjectDeps = !opts.dedupeDirectDeps ? {} : (depGraph.directDependenciesByImporterId['.'] ?? {})
  await Promise.all(selectedProjects.map(async (project) => linkBinsOfProject(project, { built, depGraph, opts, rootProjectDeps })))
}

async function linkBinsOfProject (project: Project, ctx: ProjectBinsContext): Promise<void> {
  const { built, depGraph, opts, rootProjectDeps } = ctx
  const projectModulesDir = await getProjectNodePath(project, opts)
  const protectedBins = new Set<string>()
  const linkBinOptions: LinkBinOptions = {
    extraNodePaths: opts.extraNodePaths,
    linkedCommandNames: protectedBins,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    projectModulesDir,
  }
  if (opts.nodeLinker === 'hoisted' || opts.publicHoistPattern?.length && path.relative(opts.lockfileDir, project.rootDir) === '') {
    await linkBinsOfImporter(project, {
      ...linkBinOptions,
      forceForPackages: getBuiltImporterBinDirs(project, ctx),
    })
  } else {
    await linkBinsOfDirectDeps(project, {
      depGraph,
      linkBinOptions: { ...linkBinOptions, forceForPackages: built?.builtPackageDirs },
      rootProjectDeps,
    })
  }
  if (opts.autoInstallPeers !== false && opts.nodeLinker !== 'hoisted') {
    await linkAutoInstalledPeerBins(project, { built, depGraph, linkBinOptions, protectedBins })
  }
  if (path.resolve(project.binsDir) === path.resolve(project.modulesDir, '.bin')) {
    await pruneStaleBins(project.binsDir, protectedBins)
  }
}

async function linkAutoInstalledPeerBins (
  project: Project,
  {
    built,
    depGraph,
    linkBinOptions,
    protectedBins,
  }: Pick<ProjectBinsContext, 'built' | 'depGraph'> & { linkBinOptions: LinkBinOptions, protectedBins: Set<string> }
): Promise<void> {
  const { peerPkgDirs, forceForPackages } = autoInstalledPeerBinDirs(
    depGraph.directDependenciesByImporterId[project.id],
    project.id,
    depGraph.filteredLockfile,
    built
  )
  if (peerPkgDirs.length === 0) return
  await linkBinsOfPackages(await readBinPackageManifests(peerPkgDirs), project.binsDir, {
    ...linkBinOptions,
    excludeBins: protectedBins,
    forceForPackages,
  })
}

function getBuiltImporterBinDirs (
  project: Project,
  { built, depGraph, opts }: Pick<ProjectBinsContext, 'built' | 'depGraph' | 'opts'>
): Set<string> | undefined {
  if (!built?.builtDepPaths?.size && !built?.builtPackageDirs?.size) return undefined
  const dirs = getBuiltDirectAliasDirs(project, depGraph.directDependenciesByImporterId[project.id], built.builtPackageDirs)
  if (opts.nodeLinker === 'hoisted') {
    addBuiltHoistedLocationDirs(dirs, built.builtDepPaths, depGraph.hoistedLocations, opts.lockfileDir)
  } else {
    addBuiltPublicHoistDirs(dirs, project.modulesDir, built.builtDepPaths, built.newHoistedDependencies ?? opts.hoistedDependencies)
  }
  return dirs
}

function getBuiltDirectAliasDirs (
  project: Project,
  directDeps: Record<string, string> | undefined,
  builtPackageDirs: ReadonlySet<string> | undefined
): Set<string> {
  const dirs = new Set<string>()
  if (!builtPackageDirs?.size || directDeps == null) return dirs
  for (const [alias, dir] of Object.entries(directDeps)) {
    if (builtPackageDirs.has(path.normalize(dir))) {
      dirs.add(path.normalize(safeJoinModulesDir(project.modulesDir, alias)))
    }
  }
  return dirs
}

function addBuiltHoistedLocationDirs (
  dirs: Set<string>,
  builtDepPaths: ReadonlySet<DepPath> | undefined,
  hoistedLocations: Record<string, string[]> | undefined,
  lockfileDir: string
): void {
  for (const depPath of builtDepPaths ?? []) {
    for (const hoistedLocation of hoistedLocations?.[depPath] ?? []) {
      dirs.add(path.normalize(path.join(lockfileDir, hoistedLocation)))
    }
  }
}

function addBuiltPublicHoistDirs (
  dirs: Set<string>,
  modulesDir: string,
  builtDepPaths: ReadonlySet<DepPath> | undefined,
  hoistedDependencies: HoistedDependencies
): void {
  for (const depPath of builtDepPaths ?? []) {
    for (const [alias, visibility] of Object.entries(hoistedDependencies[depPath] ?? {})) {
      if (visibility === 'public') {
        dirs.add(path.normalize(safeJoinModulesDir(modulesDir, alias)))
      }
    }
  }
}

async function linkBinsOfDirectDeps (
  project: Project,
  { depGraph, linkBinOptions, rootProjectDeps }: Pick<ProjectBinsContext, 'depGraph' | 'rootProjectDeps'> & { linkBinOptions: LinkBinOptions }
): Promise<void> {
  const directDeps = depGraph.directDependenciesByImporterId[project.id]
  const directPkgDirs = project.id === '.'
    ? Object.values(directDeps)
    : Object.entries(directDeps)
      .filter(([alias, dir]) =>
        rootProjectDeps[alias] !== dir ||
        (depGraph.graph[dir] != null && dp.isRuntimeDepPath(depGraph.graph[dir].depPath))
      )
      .map(([, dir]) => dir)
  // Skip packages without bins to avoid unnecessary manifest reads.
  // Dirs not in graph (e.g. link: deps) are kept since they may expose bins.
  const pkgDirsWithBins = directPkgDirs.filter((dir) =>
    depGraph.graph[dir] == null ||
    depGraph.graph[dir].hasBin ||
    dp.isRuntimeDepPath(depGraph.graph[dir].depPath)
  )
  await linkBinsOfPackages(await readBinPackageManifests(pkgDirsWithBins), project.binsDir, linkBinOptions)
}

async function readBinPackageManifests (pkgDirs: string[]): Promise<Array<{ location: string, manifest: DependencyManifest }>> {
  return (
    await Promise.all(
      pkgDirs.map(async (dir) => ({
        location: dir,
        manifest: await safeReadPublishManifest(dir),
      }))
    )
  )
    .filter(({ manifest }) => manifest != null) as Array<{ location: string, manifest: DependencyManifest }>
}

function autoInstalledPeerBinDirs (
  directPkgDirs: Record<string, string>,
  importerId: ProjectId,
  lockfile: LockfileObject,
  built?: ImporterBuiltBinContext
): { peerPkgDirs: string[], forceForPackages: Set<string> | undefined } {
  const peerDirs = new Set<string>()
  const builtPeerDirs = new Set<string>()
  const importer = lockfile.importers[importerId]
  const directRefs = { ...importer.dependencies, ...importer.devDependencies, ...importer.optionalDependencies }
  for (const [alias, dir] of Object.entries(directPkgDirs)) {
    const depPath = directRefs[alias] ? dp.refToRelative(directRefs[alias], alias) : null
    if (depPath == null) continue
    collectDirectDepPeerBins(dir, depPath, lockfile, built, peerDirs, builtPeerDirs)
  }
  return {
    peerPkgDirs: Array.from(peerDirs).sort(),
    forceForPackages: builtPeerDirs.size > 0 ? builtPeerDirs : undefined,
  }
}

function collectDirectDepPeerBins (
  dir: string,
  depPath: DepPath,
  lockfile: LockfileObject,
  built: ImporterBuiltBinContext | undefined,
  peerDirs: Set<string>,
  builtPeerDirs: Set<string>
): void {
  const parent = path.dirname(dir)
  const modulesDir = path.basename(parent).startsWith('@') ? path.dirname(parent) : parent
  for (const [peerName, peerDepPath] of listInstalledRequiredPeers(lockfile, depPath)) {
    const peerDir = safeJoinModulesDir(modulesDir, peerName)
    peerDirs.add(peerDir)
    const normalizedPeerDir = path.normalize(peerDir)
    if (isBuiltPeerBinDir(normalizedPeerDir, peerDepPath, built)) {
      builtPeerDirs.add(normalizedPeerDir)
    }
  }
}

function isBuiltPeerBinDir (
  normalizedPeerDir: string,
  peerDepPath: DepPath | null,
  built?: ImporterBuiltBinContext
): boolean {
  if (built == null) return false
  return Boolean(built.builtPackageDirs?.has(normalizedPeerDir) || (peerDepPath != null && built.builtDepPaths?.has(peerDepPath)))
}

/** The non-optional peers of a package that the lockfile installs next to it. */
function listInstalledRequiredPeers (lockfile: LockfileObject, depPath: DepPath): Array<[string, DepPath | null]> {
  const metadata = lockfile.packages?.[depPath]
  return Object.keys(metadata?.peerDependencies ?? {})
    .filter((peerName) =>
      !metadata?.peerDependenciesMeta?.[peerName]?.optional &&
      (metadata?.dependencies?.[peerName] != null || metadata?.optionalDependencies?.[peerName] != null)
    )
    .map((peerName) => {
      const peerRef = metadata?.dependencies?.[peerName] ?? metadata?.optionalDependencies?.[peerName]
      return [peerName, peerRef ? dp.refToRelative(peerRef, peerName) : null]
    })
}

async function linkBinsOfImporter (
  { manifest, modulesDir, binsDir, rootDir }: {
    binsDir: string
    manifest: ProjectManifest
    modulesDir: string
    rootDir: ProjectRootDir
  },
  linkBinOptions: LinkBinOptions = {}
): Promise<string[]> {
  const warn = (message: string) => {
    logger.info({ message, prefix: rootDir })
  }
  return linkBins(modulesDir, binsDir, {
    ...linkBinOptions,
    allowExoticManifests: true,
    projectManifest: manifest,
    warn,
  })
}

export async function linkRuntimeBinsOfImporters (opts: {
  directDependenciesByImporterId: DirectDependenciesByImporterId
  extraNodePaths?: string[]
  graph: DependenciesGraph
  preferSymlinkedExecutables?: boolean
  projects: Project[]
}
): Promise<void> {
  await Promise.all(opts.projects.map((project) => limitLinking(() =>
    linkBinsOfRuntimeDependencies(
      Object.values(opts.directDependenciesByImporterId[project.id]).map((location) => opts.graph[location]),
      project.binsDir,
      {
        extraNodePaths: opts.extraNodePaths,
        preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      }
    )
  )))
}
