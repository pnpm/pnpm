import { promises as fs } from 'fs'
import path from 'path'
import { calcDepState, type DepsStateCache } from '@pnpm/calc-dep-state'
import {
  progressLogger,
  stageLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import {
  filterLockfileByImporters,
} from '@pnpm/filter-lockfile'
import { linkDirectDeps } from '@pnpm/pkg-manager.direct-dep-linker'
import { type InstallationResultStats } from '@pnpm/headless'
import { hoist } from '@pnpm/hoist'
import { type Lockfile } from '@pnpm/lockfile-file'
import { logger } from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import { type IncludedDependencies } from '@pnpm/modules-yaml'
import {
  type DependenciesGraph,
  type DependenciesGraphNode,
  type LinkedDependency,
} from '@pnpm/resolve-dependencies'
import { type StoreController, type TarballResolution } from '@pnpm/store-controller-types'
import { symlinkDependency } from '@pnpm/symlink-dependency'
import {
  type HoistedDependencies,
  type Registries,
} from '@pnpm/types'
import pLimit from 'p-limit'
import pathExists from 'path-exists'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import difference from 'ramda/src/difference'
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'
import pickBy from 'ramda/src/pickBy'
import props from 'ramda/src/props'
import { type ImporterToUpdate } from './index'
import { refIsLocalDirectory } from '@pnpm/lockfile-utils'

const brokenModulesLogger = logger('_broken_node_modules')

export async function linkPackages (
  projects: ImporterToUpdate[],
  depGraph: DependenciesGraph,
  opts: {
    currentLockfile: Lockfile
    dedupeDirectDeps: boolean
    dependenciesByProjectId: Record<string, Record<string, string>>
    finishLockfileUpdates: () => Promise<unknown>
    force: boolean
    depsStateCache: DepsStateCache
    extraNodePaths: string[]
    hoistedDependencies: HoistedDependencies
    hoistedModulesDir: string
    hoistPattern?: string[]
    ignoreScripts: boolean
    publicHoistPattern?: string[]
    include: IncludedDependencies
    linkedDependenciesByProjectId: Record<string, LinkedDependency[]>
    lockfileDir: string
    makePartialCurrentLockfile: boolean
    outdatedDependencies: Record<string, string>
    pruneStore: boolean
    pruneVirtualStore: boolean
    registries: Registries
    rootModulesDir: string
    sideEffectsCacheRead: boolean
    symlink: boolean
    skipped: Set<string>
    storeController: StoreController
    virtualStoreDir: string
    wantedLockfile: Lockfile
    wantedToBeSkippedPackageIds: Set<string>
  }
): Promise<{
    currentLockfile: Lockfile
    newDepPaths: string[]
    newHoistedDependencies: HoistedDependencies
    removedDepPaths: Set<string>
    stats: InstallationResultStats
  }> {
  let depNodes = Object.values(depGraph).filter(({ depPath, id }) => {
    if (((opts.wantedLockfile.packages?.[depPath]) != null) && !opts.wantedLockfile.packages[depPath].optional) {
      opts.skipped.delete(depPath)
      return true
    }
    if (opts.wantedToBeSkippedPackageIds.has(id)) {
      opts.skipped.add(depPath)
      return false
    }
    opts.skipped.delete(depPath)
    return true
  })
  if (!opts.include.dependencies) {
    depNodes = depNodes.filter(({ dev, optional }) => dev || optional)
  }
  if (!opts.include.devDependencies) {
    depNodes = depNodes.filter(({ optional, prod }) => prod || optional)
  }
  if (!opts.include.optionalDependencies) {
    depNodes = depNodes.filter(({ optional }) => !optional)
  }
  depGraph = Object.fromEntries(depNodes.map((depNode) => [depNode.depPath, depNode]))
  const removedDepPaths = await prune(projects, {
    currentLockfile: opts.currentLockfile,
    hoistedDependencies: opts.hoistedDependencies,
    hoistedModulesDir: (opts.hoistPattern != null) ? opts.hoistedModulesDir : undefined,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    pruneStore: opts.pruneStore,
    pruneVirtualStore: opts.pruneVirtualStore,
    publicHoistedModulesDir: (opts.publicHoistPattern != null) ? opts.rootModulesDir : undefined,
    registries: opts.registries,
    skipped: opts.skipped,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    wantedLockfile: opts.wantedLockfile,
  })

  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_started',
  })

  const projectIds = projects.map(({ id }) => id)
  const filterOpts = {
    include: opts.include,
    registries: opts.registries,
    skipped: opts.skipped,
  }
  const newCurrentLockfile = filterLockfileByImporters(opts.wantedLockfile, projectIds, {
    ...filterOpts,
    failOnMissingDependencies: true,
    skipped: new Set(),
  })

  let currentLockfile: Lockfile
  const allImportersIncluded = equals(projectIds.sort(), Object.keys(opts.wantedLockfile.importers).sort())
  if (
    opts.makePartialCurrentLockfile ||
    !allImportersIncluded
  ) {
    const packages = opts.currentLockfile.packages ?? {}
    if (opts.wantedLockfile.packages != null) {
      for (const depPath in opts.wantedLockfile.packages) { // eslint-disable-line:forin
        if (depGraph[depPath]) {
          packages[depPath] = opts.wantedLockfile.packages[depPath]
        }
      }
    }
    const projects = {
      ...opts.currentLockfile.importers,
      ...pick(projectIds, opts.wantedLockfile.importers),
    }
    currentLockfile = filterLockfileByImporters(
      {
        ...opts.wantedLockfile,
        importers: projects,
        packages,
      },
      Object.keys(projects), {
        ...filterOpts,
        failOnMissingDependencies: false,
        skipped: new Set(),
      }
    )
  } else if (
    opts.include.dependencies &&
    opts.include.devDependencies &&
    opts.include.optionalDependencies &&
    opts.skipped.size === 0
  ) {
    currentLockfile = opts.wantedLockfile
  } else {
    currentLockfile = newCurrentLockfile
  }

  const filteredLockfile = filterLockfileByImporters(opts.currentLockfile, projectIds, {
    ...filterOpts,
    failOnMissingDependencies: false,
  })
  const { newDepPaths, wantedRelDepPaths } = await calculateDepPaths({
    currentLockfile: filteredLockfile,
    depGraph,
    force: opts.force,
    skipped: opts.skipped,
    wantedLockfile: newCurrentLockfile,
  })

  let linkedToRoot = 0
  let dedupeLinkedDirectDeps = async () => {}
  if (opts.symlink) {
    const projectsToLink = Object.fromEntries(await Promise.all(
      projects.map(async ({ id, manifest, modulesDir, rootDir }) => {
        const deps = opts.dependenciesByProjectId[id]
        const importerFromLockfile = newCurrentLockfile.importers[id]
        return [id, {
          dir: rootDir,
          modulesDir,
          dependencies: [
            ...Object.entries(deps)
              .filter(([rootAlias]) => importerFromLockfile.specifiers[rootAlias])
              .map(([rootAlias, depPath]) => ({ rootAlias, depGraphNode: depGraph[depPath] }))
              .filter(({ depGraphNode }) => depGraphNode)
              .map(({ rootAlias, depGraphNode }) => {
                const isDev = Boolean(manifest.devDependencies?.[depGraphNode.name])
                const isOptional = Boolean(manifest.optionalDependencies?.[depGraphNode.name])
                return {
                  alias: rootAlias,
                  name: depGraphNode.name,
                  version: depGraphNode.version,
                  dir: depGraphNode.dir,
                  id: depGraphNode.id,
                  dependencyType: (isDev && 'dev' || isOptional && 'optional' || 'prod') as 'dev' | 'optional' | 'prod',
                  latest: opts.outdatedDependencies[depGraphNode.id],
                  isExternalLink: false,
                }
              }),
            ...opts.linkedDependenciesByProjectId[id].map((linkedDependency) => {
              const dir = resolvePath(rootDir, linkedDependency.resolution.directory)
              return {
                alias: linkedDependency.alias,
                name: linkedDependency.name,
                version: linkedDependency.version,
                dir,
                id: linkedDependency.resolution.directory,
                dependencyType: (linkedDependency.dev && 'dev' || linkedDependency.optional && 'optional' || 'prod') as 'dev' | 'optional' | 'prod',
                isExternalLink: true,
              }
            }),
          ],
        }]
      }))
    )
    const dedupe = opts.dedupeDirectDeps
    const _linkDirectDeps = linkDirectDeps.bind(null, projectsToLink, { dedupe })
    linkedToRoot = await _linkDirectDeps()
    if (dedupe) {
      dedupeLinkedDirectDeps = async () => {
        await _linkDirectDeps()
      }
    }
  }
  await opts.finishLockfileUpdates()
  const { added } = await linkNewPackages(
    filteredLockfile,
    newCurrentLockfile,
    depGraph,
    {
      force: opts.force,
      depsStateCache: opts.depsStateCache,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      newDepPaths,
      optional: opts.include.optionalDependencies,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      symlink: opts.symlink,
      storeController: opts.storeController,
      virtualStoreDir: opts.virtualStoreDir,
      wantedRelDepPaths,
    }
  )

  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_done',
  })

  let newHoistedDependencies!: HoistedDependencies
  if (opts.hoistPattern == null && opts.publicHoistPattern == null) {
    newHoistedDependencies = {}
  } else if (newDepPaths.length > 0 || removedDepPaths.size > 0) {
    // It is important to keep the skipped packages in the lockfile which will be saved as the "current lockfile".
    // pnpm is comparing the current lockfile to the wanted one and they should match.
    // But for hoisting, we need a version of the lockfile w/o the skipped packages, so we're making a copy.
    const hoistLockfile = {
      ...currentLockfile,
      packages: omit(Array.from(opts.skipped), currentLockfile.packages),
    }
    newHoistedDependencies = await hoist({
      extraNodePath: opts.extraNodePaths,
      lockfile: hoistLockfile,
      importerIds: projectIds,
      privateHoistedModulesDir: opts.hoistedModulesDir,
      privateHoistPattern: opts.hoistPattern ?? [],
      publicHoistedModulesDir: opts.rootModulesDir,
      publicHoistPattern: opts.publicHoistPattern ?? [],
      virtualStoreDir: opts.virtualStoreDir,
    })
  } else {
    newHoistedDependencies = opts.hoistedDependencies
  }

  // Possibly dedupe dependencies that were hoisted after local link
  await dedupeLinkedDirectDeps()

  return {
    currentLockfile,
    newDepPaths,
    newHoistedDependencies,
    removedDepPaths,
    stats: {
      added,
      removed: removedDepPaths.size,
      linkedToRoot,
    },
  }
}

async function calculateDepPaths (opts: {
  currentLockfile: Lockfile
  depGraph: DependenciesGraph
  force: boolean
  skipped: Set<string>
  wantedLockfile: Lockfile
}): Promise<{
    newDepPaths: string[]
    wantedRelDepPaths: string[]
  }> {
  const wantedRelDepPaths = difference(Object.keys(opts.wantedLockfile.packages ?? {}), Array.from(opts.skipped))

  let newDepPathsSet: Set<string>
  if (opts.force) {
    newDepPathsSet = new Set(
      wantedRelDepPaths
      // when installing a new package, not all the nodes are analyzed
      // just skip the ones that are in the lockfile but were not analyzed
        .filter((depPath) => opts.depGraph[depPath])
    )
  } else {
    newDepPathsSet = await selectNewFromWantedDeps(wantedRelDepPaths, opts.currentLockfile, opts.depGraph)
  }

  return {
    newDepPaths: Array.from(newDepPathsSet),
    wantedRelDepPaths,
  }
}

const isAbsolutePath = /^[/]|^[A-Za-z]:/

// This function is copied from @pnpm/local-resolver
function resolvePath (where: string, spec: string) {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}

async function linkNewPackages (
  currentLockfile: Lockfile,
  wantedLockfile: Lockfile,
  depGraph: DependenciesGraph,
  opts: {
    depsStateCache: DepsStateCache
    force: boolean
    newDepPaths: string[]
    optional: boolean
    ignoreScripts: boolean
    lockfileDir: string
    sideEffectsCacheRead: boolean
    symlink: boolean
    storeController: StoreController
    virtualStoreDir: string
    wantedRelDepPaths: string[]
  }
): Promise<{ added: number }> {
  const newDepPathsSet = new Set(opts.newDepPaths)

  const added = newDepPathsSet.size
  statsLogger.debug({
    added,
    prefix: opts.lockfileDir,
  })

  const existingWithUpdatedDeps = []
  if (!opts.force && (currentLockfile.packages != null) && (wantedLockfile.packages != null)) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const depPath of opts.wantedRelDepPaths) {
      if (currentLockfile.packages[depPath] &&
        (!equals(currentLockfile.packages[depPath].dependencies, wantedLockfile.packages[depPath].dependencies) ||
        !equals(currentLockfile.packages[depPath].optionalDependencies, wantedLockfile.packages[depPath].optionalDependencies))) {
        // TODO: come up with a test that triggers the usecase of depGraph[depPath] undefined
        // see related issue: https://github.com/pnpm/pnpm/issues/870
        if (depGraph[depPath] && !newDepPathsSet.has(depPath)) {
          existingWithUpdatedDeps.push(depGraph[depPath])
        }
      }
    }
  }

  if (!newDepPathsSet.size && (existingWithUpdatedDeps.length === 0)) return { added }

  const newPkgs = props<string, DependenciesGraphNode>(opts.newDepPaths, depGraph)

  await Promise.all(newPkgs.map(async (depNode) => fs.mkdir(depNode.modules, { recursive: true })))
  await Promise.all([
    !opts.symlink
      ? Promise.resolve()
      : linkAllModules([...newPkgs, ...existingWithUpdatedDeps], depGraph, {
        lockfileDir: opts.lockfileDir,
        optional: opts.optional,
      }),
    linkAllPkgs(opts.storeController, newPkgs, {
      depGraph,
      depsStateCache: opts.depsStateCache,
      force: opts.force,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
    }),
  ])

  return { added }
}

async function selectNewFromWantedDeps (
  wantedRelDepPaths: string[],
  currentLockfile: Lockfile,
  depGraph: DependenciesGraph
) {
  const newDeps = new Set<string>()
  const prevDeps = currentLockfile.packages ?? {}
  await Promise.all(
    wantedRelDepPaths.map(
      async (depPath: string) => {
        const depNode = depGraph[depPath]
        if (!depNode) return
        const prevDep = prevDeps[depPath]
        if (
          prevDep &&
          // Local file should always be treated as a new dependency
          // https://github.com/pnpm/pnpm/issues/5381
          !refIsLocalDirectory(depNode.depPath) &&
          (depNode.resolution as TarballResolution).integrity === (prevDep.resolution as TarballResolution).integrity
        ) {
          if (await pathExists(depNode.dir)) {
            return
          }
          brokenModulesLogger.debug({
            missing: depNode.dir,
          })
        }
        newDeps.add(depPath)
      }
    )
  )
  return newDeps
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: {
    depGraph: DependenciesGraph
    depsStateCache: DepsStateCache
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    sideEffectsCacheRead: boolean
  }
) {
  return Promise.all(
    depNodes.map(async (depNode) => {
      const { files } = await depNode.fetching()

      if (typeof depNode.requiresBuild === 'function') {
        depNode.requiresBuild = await depNode.requiresBuild()
      }
      let sideEffectsCacheKey: string | undefined
      if (opts.sideEffectsCacheRead && files.sideEffects && !isEmpty(files.sideEffects)) {
        sideEffectsCacheKey = calcDepState(opts.depGraph, opts.depsStateCache, depNode.depPath, {
          isBuilt: !opts.ignoreScripts && depNode.requiresBuild,
          patchFileHash: depNode.patchFile?.hash,
        })
      }
      const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
        filesResponse: files,
        force: opts.force,
        sideEffectsCacheKey,
        requiresBuild: depNode.requiresBuild || depNode.patchFile != null,
      })
      if (importMethod) {
        progressLogger.debug({
          method: importMethod,
          requester: opts.lockfileDir,
          status: 'imported',
          to: depNode.dir,
        })
      }
      depNode.isBuilt = isBuilt

      const selfDep = depNode.children[depNode.name]
      if (selfDep) {
        const pkg = opts.depGraph[selfDep]
        if (!pkg || !pkg.installable && pkg.optional) return
        const targetModulesDir = path.join(depNode.modules, depNode.name, 'node_modules')
        await limitLinking(async () => symlinkDependency(pkg.dir, targetModulesDir, depNode.name))
      }
    })
  )
}

async function linkAllModules (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    lockfileDir: string
    optional: boolean
  }
) {
  await Promise.all(
    depNodes
      .map(async ({ children, optionalDependencies, name, modules }) => {
        const childrenToLink: Record<string, string> = opts.optional
          ? children
          : pickBy((_, childAlias) => !optionalDependencies.has(childAlias), children)

        await Promise.all(
          Object.entries(childrenToLink)
            .map(async ([childAlias, childDepPath]) => {
              if (childDepPath.startsWith('link:')) {
                await limitLinking(() => symlinkDependency(path.resolve(opts.lockfileDir, childDepPath.slice(5)), modules, childAlias))
                return
              }
              const pkg = depGraph[childDepPath]
              if (!pkg || !pkg.installable && pkg.optional || childAlias === name) return
              await limitLinking(() => symlinkDependency(pkg.dir, modules, childAlias))
            })
        )
      })
  )
}
