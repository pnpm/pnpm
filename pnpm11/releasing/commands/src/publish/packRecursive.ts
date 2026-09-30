import path from 'node:path'

import { getWorkspaceConcurrency } from '@pnpm/config.reader'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { logger } from '@pnpm/logger'
import type { Project, ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'

import type { PackOptions, PackResultJson } from './pack.js'
import { api, resolvePackOutput, toPackResultJson } from './packApi.js'

export type PackDestinationLocker = (destination: string, write: () => Promise<void>) => Promise<void>

/**
 * Packs every selected workspace project that has a name and a version, in dependency order,
 * and returns their results in that order.
 */
export async function packRecursively (opts: PackOptions): Promise<PackResultJson[]> {
  const selectedProjectsGraph = opts.selectedProjectsGraph as ProjectsGraph
  const packedPkgDirs = selectPackablePkgDirs(selectedProjectsGraph)

  if (packedPkgDirs.size === 0) {
    logger.info({
      message: 'There are no packages that should be packed',
      prefix: opts.dir,
    })
  }

  const projectDependencies = filteredProjectsDependencies({
    selectedProjectsGraph,
    allProjectsGraph: opts.allProjectsGraph,
    prodAllProjectsGraph: opts.prodAllProjectsGraph,
    prodOnlySelectedProjectDirs: opts.prodOnlySelectedProjectDirs,
  })

  const resolvedOpts = resolvePackDestinationOptions(opts)
  const packOrder = serializeSharedPackDestinations({
    opts: resolvedOpts,
    packedPkgDirs,
    projectDependencies,
    projectsGraph: selectedProjectsGraph,
  })
  resolvedOpts.packDestinationLocker = createPackDestinationLocker()
  const packedByDir = await packInGraphOrder({
    opts: resolvedOpts,
    packedPkgDirs,
    projectDependencies,
    projectsGraph: selectedProjectsGraph,
  })
  const packedPackages: PackResultJson[] = []
  for (const pkgDir of packOrder) {
    const result = packedByDir.get(pkgDir)
    if (result != null) packedPackages.push(result)
  }
  return packedPackages
}

function selectPackablePkgDirs (selectedProjectsGraph: ProjectsGraph): Set<ProjectRootDir> {
  const pkgsToPack: Project[] = []
  for (const { package: pkg } of Object.values(selectedProjectsGraph)) {
    if (pkg.manifest.name && pkg.manifest.version) {
      pkgsToPack.push(pkg)
    }
  }
  return new Set<ProjectRootDir>(pkgsToPack.map(({ rootDir }) => rootDir))
}

function resolvePackDestinationOptions (opts: PackOptions): PackOptions {
  const resolvedOpts = { ...opts }
  if (opts.out) {
    resolvedOpts.out = path.resolve(opts.dir, opts.out)
  } else if (opts.packDestination) {
    resolvedOpts.packDestination = path.resolve(opts.dir, opts.packDestination)
  } else {
    resolvedOpts.packDestination = path.resolve(opts.dir)
  }
  return resolvedOpts
}

interface PackGraphParams {
  opts: PackOptions
  packedPkgDirs: Set<ProjectRootDir>
  projectDependencies: Map<ProjectRootDir, ProjectRootDir[]>
  projectsGraph: ProjectsGraph
}

async function packInGraphOrder ({ opts, packedPkgDirs, projectDependencies, projectsGraph }: PackGraphParams): Promise<Map<ProjectRootDir, PackResultJson>> {
  let firstError: unknown
  const packedByDir = new Map<ProjectRootDir, PackResultJson>()
  await scheduleGraph(projectDependencies, {
    bail: true,
    concurrency: getWorkspaceConcurrency(opts.workspaceConcurrency),
    runNode: async (pkgDir): Promise<TaskCompletion> => {
      try {
        if (!packedPkgDirs.has(pkgDir)) return 'passed'
        const pkg = projectsGraph[pkgDir].package
        const packResult = await api({
          ...opts,
          dir: pkg.rootDir,
        })
        packedByDir.set(pkgDir, toPackResultJson(packResult))
        return 'passed'
      } catch (error: unknown) {
        firstError ??= error
        return 'aborted'
      }
    },
    onNodeSkipped: () => {},
  })
  if (firstError != null) throw firstError
  return packedByDir
}

/**
 * Makes each project that writes to the same tarball path as an earlier project in pack order
 * depend on that project, so the two never write the same file at once. Returns the pack order.
 */
function serializeSharedPackDestinations (params: PackGraphParams): ProjectRootDir[] {
  const { opts, packedPkgDirs, projectDependencies, projectsGraph } = params
  const order = graphSequencer(projectDependencies).order
  if (canPackOutputChangeWhilePacking(opts, order, projectsGraph) && !isLiteralPackOutput(opts.out)) return order
  const previousByOutput = new Map<string, ProjectRootDir>()
  for (const pkgDir of order) {
    if (!packedPkgDirs.has(pkgDir)) continue
    const output = packOutputPath(opts, projectsGraph[pkgDir].package)
    if (output == null) continue
    const predecessor = previousByOutput.get(output)
    previousByOutput.set(output, pkgDir)
    if (predecessor != null && !projectDependencies.get(pkgDir)!.includes(predecessor)) {
      projectDependencies.get(pkgDir)!.push(predecessor)
    }
  }
  return order
}

function canPackOutputChangeWhilePacking (opts: PackOptions, order: ProjectRootDir[], projectsGraph: ProjectsGraph): boolean {
  return opts.hooks?.beforePacking != null || order.some((pkgDir) => {
    const manifest = projectsGraph[pkgDir].package.manifest
    return manifest.publishConfig?.directory != null || (!opts.ignoreScripts &&
      ['prepack', 'prepare'].some((script) => Boolean(manifest.scripts?.[script])))
  })
}

function isLiteralPackOutput (out: string | undefined): boolean {
  return out != null && !out.includes('%s') && !out.includes('%v')
}

function createPackDestinationLocker (): PackDestinationLocker {
  const pending = new Map<string, Promise<void>>()
  return async (destination, write) => {
    const previous = pending.get(destination)
    let release!: () => void
    const current = new Promise<void>((resolve) => {
      release = resolve
    })
    pending.set(destination, current)
    await previous
    try {
      await write()
    } finally {
      release()
      if (pending.get(destination) === current) pending.delete(destination)
    }
  }
}

function packOutputPath (opts: PackOptions, pkg: Project): string | undefined {
  const publishedName = pkg.manifest.publishConfig?.name ?? pkg.manifest.name
  const publishedVersion = pkg.manifest.version
  if (publishedName == null || publishedVersion == null) return undefined
  return resolvePackOutput({
    dir: pkg.rootDir,
    out: opts.out,
    packDestination: opts.packDestination,
    publishedName,
    publishedVersion,
  }).outputPath
}
