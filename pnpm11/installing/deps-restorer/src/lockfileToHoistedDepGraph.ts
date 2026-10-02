import path from 'node:path'

import type {
  DependenciesGraph,
  DepHierarchy,
  DirectDependenciesByImporterId,
  LockfileToDepGraphResult,
} from '@pnpm/deps.graph-builder'
import { hoist, type HoisterResult, type HoistingLimits } from '@pnpm/installing.linking.real-hoist'
import type { IncludedDependencies } from '@pnpm/installing.modules-yaml'
import type {
  LockfileObject,
  ProjectSnapshot,
} from '@pnpm/lockfile.fs'
import type { PatchGroupRecord } from '@pnpm/patching.config'
import type { StoreController } from '@pnpm/store.controller-types'
import type { AllowBuild, DepPath, ProjectId, RegistryContext, SupportedArchitectures } from '@pnpm/types'
import { pathAbsolute } from 'path-absolute'

import { pickResolvableImporterIds } from './currentLockfileImporters.js'
import { fetchDeps, type FetchDepsOptions, type SkipFetchingOption } from './fetchHoistedDeps.js'

export interface LockfileToHoistedDepGraphOptions extends RegistryContext {
  allowBuild?: AllowBuild
  autoInstallPeers: boolean
  engineStrict: boolean
  force: boolean
  /** See `installabilityUnderForce` in `@pnpm/config.package-is-installable`. */
  includeIncompatiblePackages?: boolean
  hoistingLimits?: HoistingLimits
  externalDependencies?: Set<string>
  importerIds: string[]
  include: IncludedDependencies
  ignoreScripts: boolean
  /**
   * When true, skip fetching local dependencies (file: protocol pointing to directories).
   * This is used by `pnpm fetch` which only downloads packages from the registry
   * and doesn't need local packages that won't be available (e.g., in Docker builds).
   */
  ignoreLocalPackages?: boolean
  currentHoistedLocations?: Record<string, string[]>
  lockfileDir: string
  modulesDir?: string
  nodeVersion: string
  pnpmVersion: string
  patchedDependencies?: PatchGroupRecord
  /**
   * The dep paths a non-optional edge reaches, as classified by
   * `filterLockfileByImportersAndEngine`. Installability is evaluated as
   * optional for everything outside this set.
   */
  requiredDepPaths: Set<DepPath>
  /**
   * An importer whose node_modules is the root node_modules while the root
   * project is not installed. Its dependencies are hoisted as the root's, so
   * each of its direct dependencies takes the top-level slot.
   */
  rootImporterId?: ProjectId
  sideEffectsCacheRead: boolean
  skipped: Set<string>
  storeController: StoreController
  storeDir: string
  virtualStoreDir: string
  supportedArchitectures?: SupportedArchitectures
}

export async function lockfileToHoistedDepGraph (
  lockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  opts: LockfileToHoistedDepGraphOptions
): Promise<LockfileToDepGraphResult> {
  let prevGraph!: DependenciesGraph
  if (currentLockfile?.packages != null) {
    prevGraph = (await _lockfileToHoistedDepGraph(currentLockfile, {
      ...opts,
      importerIds: pickResolvableImporterIds(currentLockfile, opts.importerIds),
      force: true,
      includeIncompatiblePackages: true,
      skipFetching: true,
      skipped: new Set(),
    })).graph
  } else {
    prevGraph = {}
  }
  return {
    ...(await _lockfileToHoistedDepGraph(lockfile, opts)),
    prevGraph,
  }
}

async function _lockfileToHoistedDepGraph (
  lockfile: LockfileObject,
  opts: LockfileToHoistedDepGraphOptions & SkipFetchingOption
): Promise<Omit<LockfileToDepGraphResult, 'prevGraph'>> {
  const { tree, rootImporterId } = hoistImporters(lockfile, opts)
  const graph: DependenciesGraph = {}
  const modulesDir = pathAbsolute(opts.modulesDir ?? 'node_modules', opts.lockfileDir)
  const fetchDepsOpts: FetchDepsOptions = {
    ...opts,
    lockfile,
    graph,
    pkgLocationsByPkgId: {} as Record<string, string[]>,
    injectionTargetsByDepPath: new Map<string, string[]>(),
    hoistedLocations: {} as Record<string, string[]>,
  }
  const hierarchy = {
    [opts.lockfileDir]: await fetchDeps(fetchDepsOpts, modulesDir, tree.dependencies),
  }
  const projectsCtx: ProjectDepsContext = {
    directDependenciesByImporterId: {
      '.': directDepsMap(Object.keys(hierarchy[opts.lockfileDir]), graph),
    },
    fetchDepsOpts,
    hierarchy,
    symlinkedDirectDependenciesByImporterId: { '.': {} },
  }
  if (rootImporterId != null) {
    addRootImporterDeps(projectsCtx, rootImporterId)
  }
  await Promise.all(
    Array.from(tree.dependencies).map(async (rootDep) => addWorkspaceProjectDeps(projectsCtx, rootDep))
  )
  return {
    directDependenciesByImporterId: projectsCtx.directDependenciesByImporterId,
    graph,
    hierarchy,
    symlinkedDirectDependenciesByImporterId: projectsCtx.symlinkedDirectDependenciesByImporterId,
    hoistedLocations: fetchDepsOpts.hoistedLocations,
    injectionTargetsByDepPath: fetchDepsOpts.injectionTargetsByDepPath,
  }
}

function hoistImporters (
  lockfile: LockfileObject,
  opts: LockfileToHoistedDepGraphOptions
): { tree: HoisterResult, rootImporterId?: ProjectId } {
  const { importers, rootImporterId } = pickImportersToHoist(lockfile, opts)
  const tree = hoist({
    ...lockfile,
    importers,
  }, {
    hoistingLimits: opts.hoistingLimits,
    externalDependencies: opts.externalDependencies,
    autoInstallPeers: opts.autoInstallPeers,
  })
  return { tree, rootImporterId }
}

function pickImportersToHoist (
  lockfile: LockfileObject,
  opts: Pick<LockfileToHoistedDepGraphOptions, 'importerIds' | 'rootImporterId'>
): { importers: LockfileObject['importers'], rootImporterId?: ProjectId } {
  const importerIdsSet = opts.importerIds ? new Set(opts.importerIds) : undefined
  const importers: LockfileObject['importers'] = importerIdsSet
    ? Object.fromEntries(
      Object.entries(lockfile.importers).filter(([importerId]) => importerIdsSet.has(importerId as ProjectId))
    ) as LockfileObject['importers']
    : lockfile.importers
  const rootImporterId = opts.rootImporterId != null && importers[opts.rootImporterId] != null && importers['.' as ProjectId] == null
    ? opts.rootImporterId
    : undefined
  if (rootImporterId == null) return { importers }
  const { [rootImporterId]: rootImporter, ...otherImporters } = importers
  return {
    importers: { ...otherImporters, ['.' as ProjectId]: rootImporter },
    rootImporterId,
  }
}

interface ProjectDepsContext {
  directDependenciesByImporterId: DirectDependenciesByImporterId
  fetchDepsOpts: FetchDepsOptions
  hierarchy: Record<string, DepHierarchy>
  symlinkedDirectDependenciesByImporterId: DirectDependenciesByImporterId
}

function addRootImporterDeps (ctx: ProjectDepsContext, rootImporterId: ProjectId): void {
  const { lockfile, lockfileDir, include } = ctx.fetchDepsOpts
  ctx.directDependenciesByImporterId[rootImporterId] = ctx.directDependenciesByImporterId['.']
  ctx.symlinkedDirectDependenciesByImporterId[rootImporterId] = pickLinkedDirectDeps(
    lockfile.importers[rootImporterId],
    path.join(lockfileDir, rootImporterId),
    include
  )
}

async function addWorkspaceProjectDeps (ctx: ProjectDepsContext, rootDep: HoisterResult): Promise<void> {
  const reference = Array.from(rootDep.references)[0]
  if (!reference.startsWith('workspace:')) return
  const { fetchDepsOpts } = ctx
  const importerId = reference.replace('workspace:', '') as ProjectId
  const projectDir = path.join(fetchDepsOpts.lockfileDir, importerId)
  const modulesDir = path.join(projectDir, 'node_modules')
  const nextHierarchy = (await fetchDeps(fetchDepsOpts, modulesDir, rootDep.dependencies))
  const importer = fetchDepsOpts.lockfile.importers[importerId]
  if (importerHasDeps(importer) || rootDep.dependencies.size > 0) {
    ctx.hierarchy[projectDir] = nextHierarchy
  }

  ctx.symlinkedDirectDependenciesByImporterId[importerId] = pickLinkedDirectDeps(importer, projectDir, fetchDepsOpts.include)
  ctx.directDependenciesByImporterId[importerId] = directDepsMap(Object.keys(nextHierarchy), fetchDepsOpts.graph)
}

function importerHasDeps (importer: ProjectSnapshot): boolean {
  return [importer.dependencies, importer.devDependencies, importer.optionalDependencies]
    .some((deps) => deps != null && Object.keys(deps).length > 0)
}

function directDepsMap (directDepDirs: string[], graph: DependenciesGraph): Record<string, string> {
  const acc: Record<string, string> = {}
  for (const dir of directDepDirs) {
    acc[graph[dir].alias!] = dir
  }
  return acc
}

function pickLinkedDirectDeps (
  importer: ProjectSnapshot,
  importerDir: string,
  include: IncludedDependencies
): Record<string, string> {
  const rootDeps = {
    ...(include.devDependencies ? importer.devDependencies : {}),
    ...(include.dependencies ? importer.dependencies : {}),
    ...(include.dependencies && include.optionalDependencies ? importer.optionalDependencies : {}),
  }
  const directDeps: Record<string, string> = {}
  for (const alias in rootDeps) {
    const ref = rootDeps[alias]
    if (ref.startsWith('link:')) {
      directDeps[alias] = path.resolve(importerDir, ref.slice(5))
    }
  }
  return directDeps
}
