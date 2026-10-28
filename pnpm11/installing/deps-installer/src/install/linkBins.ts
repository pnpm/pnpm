import path from 'node:path'

import { getProjectNodePath, linkBins, linkBinsOfPackages, pruneStaleBins } from '@pnpm/bins.linker'
import { linkBinsOfDependencies, linkBinsOfRuntimeDependencies } from '@pnpm/building.during-install'
import type { DependenciesGraph, DependenciesGraphNode } from '@pnpm/installing.deps-resolver'
import { logger } from '@pnpm/logger'
import type { DependencyManifest, DepPath, HoistedDependencies } from '@pnpm/types'
import { safeReadPublishManifest } from '@pnpm/workspace.project-manifest-reader'
import pLimit from 'p-limit'
import { props } from 'ramda'

import type { LinkContext } from './linkResolvedProjects.js'
import type { ImporterToUpdate } from './mutationTypes.js'

const limitLinking = pLimit(16)

export async function linkRuntimeBinsOfImporters (opts: {
  dependenciesByProjectId: Record<string, Map<string, DepPath>>
  dependenciesGraph: DependenciesGraph
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
  projects: Array<Pick<ImporterToUpdate, 'binsDir' | 'id'>>
}
): Promise<void> {
  await Promise.all(opts.projects.map((project) => limitLinking(() =>
    linkBinsOfRuntimeDependencies(
      Array.from(opts.dependenciesByProjectId[project.id].values()).map((depPath) => opts.dependenciesGraph[depPath]),
      project.binsDir,
      {
        extraNodePaths: opts.extraNodePaths,
        preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      }
    )
  )))
}

export interface BuiltBinContext {
  builtDepPaths?: ReadonlySet<DepPath>
  builtPackageDirs?: ReadonlySet<string>
  hoistedDependencies: HoistedDependencies
  newDepPaths: DepPath[] | undefined
}

/**
 * Links the bins of the newly added packages into their dependents, and the
 * bins of each project's direct dependencies into the project.
 */
export async function linkBinsOfResolvedProjects (step: LinkContext, built: BuiltBinContext): Promise<void> {
  const { ctx, opts, projects, resolution } = step
  const { builtDepPaths, builtPackageDirs, newDepPaths } = built
  if (newDepPaths?.length && !opts.virtualStoreOnly) {
    const newPkgs = selectPackagesToLink(resolution.dependenciesGraph, {
      builtDepPaths,
      newDepPaths,
      skipped: ctx.skipped,
    })
    await linkAllBins(newPkgs, resolution.dependenciesGraph, {
      builtPackageDirs,
      extraNodePaths: ctx.extraNodePaths,
      optional: opts.include.optionalDependencies,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn: warnAboutBins.bind(null, opts.lockfileDir),
    })
  }

  if (!opts.virtualStoreOnly) await Promise.all(projects.map(async (project) => linkBinsOfProject(step, project, built)))
}

function selectPackagesToLink (
  depGraph: DependenciesGraph,
  {
    builtDepPaths,
    newDepPaths,
    skipped,
  }: {
    builtDepPaths: ReadonlySet<DepPath> | undefined
    newDepPaths: DepPath[]
    skipped: ReadonlySet<DepPath>
  }
): DependenciesGraphNode[] {
  const selected = new Set(newDepPaths.filter((depPath) => !skipped.has(depPath)))
  if (!builtDepPaths?.size) {
    return props<DepPath, DependenciesGraphNode>(Array.from(selected), depGraph)
  }
  for (const [depPath, node] of Object.entries(depGraph) as Array<[DepPath, DependenciesGraphNode]>) {
    if (!skipped.has(depPath) && Object.values(node.children).some((childDepPath) => builtDepPaths.has(childDepPath))) {
      selected.add(depPath)
    }
  }
  return props<DepPath, DependenciesGraphNode>(Array.from(selected), depGraph)
}

function warnAboutBins (prefix: string, message: string): void {
  logger.info({ message, prefix })
}

async function linkAllBins (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    builtPackageDirs?: ReadonlySet<string>
    extraNodePaths?: string[]
    preferSymlinkedExecutables?: boolean
    optional: boolean
    warn: (message: string) => void
  }
): Promise<void> {
  await Promise.all(
    depNodes.map(async depNode => limitLinking(async () => linkBinsOfDependencies(depNode, depGraph, opts)))
  )
}

async function linkBinsOfProject (step: LinkContext, project: ImporterToUpdate, built: BuiltBinContext): Promise<void> {
  const { ctx, opts } = step
  const projectModulesDir = await getProjectNodePath(project, { extendNodePath: opts.extendNodePath })
  const protectedBins = new Set<string>()
  const linkedPackages = ctx.publicHoistPattern?.length && path.relative(project.rootDir, opts.lockfileDir) === ''
    ? await linkBins(project.modulesDir, project.binsDir, {
      allowExoticManifests: true,
      forceForPackages: getBuiltPubliclyLinkedDirs(step, project, built),
      linkedCommandNames: protectedBins,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      projectManifest: project.manifest,
      extraNodePaths: ctx.extraNodePaths,
      projectModulesDir,
      warn: warnAboutBins.bind(null, project.rootDir),
    })
    : await linkBinsOfDirectDependencies(step, {
      builtPackageDirs: built.builtPackageDirs,
      linkedCommandNames: protectedBins,
      project,
      projectModulesDir,
    })
  if (!opts.global && path.resolve(project.binsDir) === path.resolve(project.modulesDir, '.bin')) {
    await pruneStaleBins(project.binsDir, protectedBins)
  }
  warnMissingGlobalBins(opts, project, linkedPackages)
}

function getBuiltPubliclyLinkedDirs (
  { resolution }: LinkContext,
  project: ImporterToUpdate,
  { builtDepPaths, hoistedDependencies }: BuiltBinContext
): Set<string> | undefined {
  if (!builtDepPaths?.size) return undefined
  const dirs = new Set<string>()
  for (const [alias, depPath] of resolution.dependenciesByProjectId[project.id].entries()) {
    if (builtDepPaths.has(depPath)) {
      dirs.add(path.normalize(path.resolve(project.modulesDir, alias)))
    }
  }
  for (const depPath of builtDepPaths) {
    for (const [alias, visibility] of Object.entries(hoistedDependencies[depPath] ?? {})) {
      if (visibility === 'public') {
        dirs.add(path.normalize(path.resolve(project.modulesDir, alias)))
      }
    }
  }
  return dirs
}

function warnMissingGlobalBins (opts: LinkContext['opts'], project: ImporterToUpdate, linkedPackages: string[] | undefined): void {
  if (!opts.global || !project.mutation.includes('install')) return
  for (const pkg of project.wantedDependencies) {
    // This warning is never printed currently during "pnpm link --global"
    // due to the following issue: https://github.com/pnpm/pnpm/issues/4761
    if (pkg.alias && !linkedPackages?.includes(pkg.alias)) {
      logger.warn({ message: `${pkg.alias} has no binaries`, prefix: opts.lockfileDir })
    }
  }
}

async function linkBinsOfDirectDependencies (
  { ctx, opts, resolution }: LinkContext,
  {
    builtPackageDirs,
    linkedCommandNames,
    project,
    projectModulesDir,
  }: {
    builtPackageDirs?: ReadonlySet<string>
    linkedCommandNames: Set<string>
    project: ImporterToUpdate
    projectModulesDir: string | undefined
  }
): Promise<string[]> {
  const directPkgs = [
    ...props<DepPath, DependenciesGraphNode>(
      Array.from(resolution.dependenciesByProjectId[project.id].values()).filter((depPath) => !ctx.skipped.has(depPath)),
      resolution.dependenciesGraph
    ),
    ...resolution.linkedDependenciesByProjectId[project.id].map(({ pkgId }) => ({
      dir: path.join(project.rootDir, pkgId.substring(5)),
      fetching: undefined,
    })),
  ]
  return linkBinsOfPackages(
    (
      await Promise.all(
        directPkgs.map(async (dep) => {
          const bundledManifest = builtPackageDirs?.has(path.normalize(dep.dir))
            ? undefined
            : (await dep.fetching?.())?.bundledManifest
          const manifest = bundledManifest ?? await safeReadPublishManifest(dep.dir)
          return {
            location: dep.dir,
            manifest,
          }
        })
      )
    )
      .filter(({ manifest }) => manifest != null) as Array<{ location: string, manifest: DependencyManifest }>,
    project.binsDir,
    {
      extraNodePaths: ctx.extraNodePaths,
      forceForPackages: builtPackageDirs,
      linkedCommandNames,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      projectModulesDir,
    }
  )
}
