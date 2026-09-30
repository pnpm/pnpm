import path from 'node:path'

import { getProjectNodePath, type LinkBinOptions, linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { linkBinsOfRuntimeDependencies } from '@pnpm/building.during-install'
import type { DependenciesGraph, DirectDependenciesByImporterId } from '@pnpm/deps.graph-builder'
import * as dp from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import type {
  DependencyManifest,
  DepPath,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
} from '@pnpm/types'
import { safeReadPublishManifest } from '@pnpm/workspace.project-manifest-reader'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'
import { limitLinking } from './limits.js'
import type { HeadlessOptions, Project } from './types.js'

interface ProjectBinsContext {
  depGraph: HeadlessDepGraph
  opts: HeadlessOptions
  rootProjectDeps: Record<string, string>
}

export async function linkBinsOfImporters ({ opts, selectedProjects }: HeadlessContext, depGraph: HeadlessDepGraph): Promise<void> {
  const rootProjectDeps = !opts.dedupeDirectDeps ? {} : (depGraph.directDependenciesByImporterId['.'] ?? {})
  await Promise.all(selectedProjects.map(async (project) => linkBinsOfProject(project, { depGraph, opts, rootProjectDeps })))
}

async function linkBinsOfProject (project: Project, { depGraph, opts, rootProjectDeps }: ProjectBinsContext): Promise<void> {
  const projectModulesDir = await getProjectNodePath(project, opts)
  const protectedBins = new Set<string>()
  const linkBinOptions = {
    extraNodePaths: opts.extraNodePaths,
    linkedCommandNames: protectedBins,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    projectModulesDir,
  }
  if (opts.nodeLinker === 'hoisted' || opts.publicHoistPattern?.length && path.relative(opts.lockfileDir, project.rootDir) === '') {
    await linkBinsOfImporter(project, linkBinOptions)
  } else {
    await linkBinsOfDirectDeps(project, { depGraph, linkBinOptions, rootProjectDeps })
  }
  if (opts.autoInstallPeers !== false && opts.nodeLinker !== 'hoisted') {
    const peerPkgDirs = autoInstalledPeerBinDirs(
      depGraph.directDependenciesByImporterId[project.id],
      project.id,
      depGraph.filteredLockfile
    )
    if (peerPkgDirs.length > 0) {
      await linkBinsOfPackages(await readBinPackageManifests(peerPkgDirs), project.binsDir, {
        excludeBins: protectedBins,
        extraNodePaths: opts.extraNodePaths,
        preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
        projectModulesDir,
      })
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
      .filter(([alias, dir]) => rootProjectDeps[alias] !== dir)
      .map(([, dir]) => dir)
  // Skip packages without bins to avoid unnecessary manifest reads.
  // Dirs not in graph (e.g. link: deps) are kept since they may expose bins.
  const pkgDirsWithBins = directPkgDirs.filter((dir) => depGraph.graph[dir] == null || depGraph.graph[dir].hasBin)
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
  lockfile: LockfileObject
): string[] {
  const peerDirs = new Set<string>()
  const importer = lockfile.importers[importerId]
  const directRefs = { ...importer.dependencies, ...importer.devDependencies, ...importer.optionalDependencies }
  for (const [alias, dir] of Object.entries(directPkgDirs)) {
    const ref = directRefs[alias]
    const depPath = ref ? dp.refToRelative(ref, alias) : null
    if (depPath == null) continue
    const parent = path.dirname(dir)
    const modulesDir = path.basename(parent).startsWith('@') ? path.dirname(parent) : parent
    for (const peerName of listInstalledRequiredPeers(lockfile, depPath)) {
      peerDirs.add(safeJoinModulesDir(modulesDir, peerName))
    }
  }
  return Array.from(peerDirs).sort()
}

/** The non-optional peers of a package that the lockfile installs next to it. */
function listInstalledRequiredPeers (lockfile: LockfileObject, depPath: DepPath): string[] {
  const metadata = lockfile.packages?.[depPath]
  return Object.keys(metadata?.peerDependencies ?? {}).filter((peerName) =>
    !metadata?.peerDependenciesMeta?.[peerName]?.optional &&
    (metadata?.dependencies?.[peerName] != null || metadata?.optionalDependencies?.[peerName] != null)
  )
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
