import path from 'node:path'

import { getWorkspaceConcurrency } from '@pnpm/config.reader'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { Project, ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'

import { publishedName } from '../publishedNames.js'
import type { PackOptions, PackResultJson } from './pack.js'
import { api, toPackResultJson } from './packApi.js'
import { canonicalizePackDestination, type PackDestinationCache, resolvePackOutput, resolveRelativeTarget } from './packOutput.js'

export type PackDestinationLocker = (destination: string, write: () => Promise<void>) => Promise<void>

/**
 * Packs every selected workspace project that has a name and a version, in dependency order,
 * and returns their results in that order.
 */
export async function packRecursively (opts: PackOptions): Promise<PackResultJson[]> {
  if (opts.out && opts.packDestination) {
    throw new PnpmError('INVALID_OPTION', 'Cannot use --pack-destination and --out together')
  }
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
  const packOrder = await serializeSharedPackDestinations({
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
  return {
    ...opts,
    outputBaseDir: resolveRelativeTarget(process.cwd(), opts.dir),
  }
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

interface PlannedPackOutput {
  destDir: string
  pkgDir: ProjectRootDir
  tarballName: string
}

/**
 * Makes each project that writes to the same tarball path as an earlier project in pack order
 * depend on that project, so the two never write the same file at once. Returns the pack order.
 */
async function serializeSharedPackDestinations (params: PackGraphParams): Promise<ProjectRootDir[]> {
  const { opts, projectDependencies, projectsGraph } = params
  const order = graphSequencer(projectDependencies).order
  if (canPackOutputChangeWhilePacking(opts, order, projectsGraph) && !isLiteralPackOutput(opts.out)) return order
  const candidates = collectCollidingPackOutputs({ ...params, order })
  if (candidates.length === 0) return order
  const destinationCache: PackDestinationCache = new Map()
  const outputs = await Promise.all(
    candidates.map(async ({ destDir, tarballName }) => path.join(
      await canonicalizePackDestination(destDir, destinationCache),
      tarballName
    ))
  )
  linkSharedDestinationPredecessors(candidates, outputs, projectDependencies)
  return order
}

function collectCollidingPackOutputs (params: PackGraphParams & { order: ProjectRootDir[] }): PlannedPackOutput[] {
  const planned: PlannedPackOutput[] = []
  for (const pkgDir of params.order) {
    if (!params.packedPkgDirs.has(pkgDir)) continue
    const item = planPackOutput(params.opts, pkgDir, params.projectsGraph[pkgDir].package)
    if (item != null) planned.push(item)
  }
  return filterRepeatedTarballNames(planned)
}

function filterRepeatedTarballNames (planned: PlannedPackOutput[]): PlannedPackOutput[] {
  const counts = new Map<string, number>()
  for (const { tarballName } of planned) {
    counts.set(tarballName, (counts.get(tarballName) ?? 0) + 1)
  }
  return planned.filter(({ tarballName }) => (counts.get(tarballName) ?? 0) > 1)
}

function linkSharedDestinationPredecessors (
  candidates: PlannedPackOutput[],
  outputs: string[],
  projectDependencies: Map<ProjectRootDir, ProjectRootDir[]>
): void {
  const previousByOutput = new Map<string, ProjectRootDir>()
  for (let index = 0; index < candidates.length; index += 1) {
    const output = outputs[index]
    const { pkgDir } = candidates[index]
    const predecessor = previousByOutput.get(output)
    previousByOutput.set(output, pkgDir)
    if (predecessor != null && !projectDependencies.get(pkgDir)!.includes(predecessor)) {
      projectDependencies.get(pkgDir)!.push(predecessor)
    }
  }
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

export function createPackDestinationLocker (): PackDestinationLocker {
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

function planPackOutput (
  opts: PackOptions,
  pkgDir: ProjectRootDir,
  pkg: Project
): PlannedPackOutput | undefined {
  const targetName = publishedName(pkg.manifest)
  const publishedVersion = pkg.manifest.version
  if (!targetName || !publishedVersion) return undefined
  try {
    const { destDir, tarballName } = resolvePackOutput({
      dir: pkg.rootDir,
      outputBaseDir: opts.outputBaseDir,
      out: opts.out,
      packDestination: opts.packDestination,
      publishedName: targetName,
      publishedVersion,
    })
    return { destDir, pkgDir, tarballName }
  } catch (err: unknown) {
    if (err instanceof PnpmError) return undefined
    throw err
  }
}
