import { promises as fs } from 'fs'
import path from 'path'
import { calcDepState, DepsStateCache } from '@pnpm/calc-dep-state'
import {
  progressLogger,
  rootLogger,
  stageLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import {
  filterLockfileByImporters,
} from '@pnpm/filter-lockfile'
import hoist from '@pnpm/hoist'
import { Lockfile } from '@pnpm/lockfile-file'
import logger from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import {
  DependenciesGraph,
  DependenciesGraphNode,
  LinkedDependency,
} from '@pnpm/resolve-dependencies'
import { StoreController } from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import {
  HoistedDependencies,
  Registries,
} from '@pnpm/types'
import pLimit from 'p-limit'
import pathExists from 'path-exists'
import fromPairs from 'ramda/src/fromPairs'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import difference from 'ramda/src/difference'
import omit from 'ramda/src/omit'
import props from 'ramda/src/props'
import { ImporterToUpdate } from './index'

const brokenModulesLogger = logger('_broken_node_modules')

export default async function linkPackages (
  projects: ImporterToUpdate[],
  depGraph: DependenciesGraph,
  opts: {
    currentLockfile: Lockfile
    dependenciesByProjectId: {
      [id: string]: {[alias: string]: string}
    }
    force: boolean
    depsStateCache: DepsStateCache
    hoistedDependencies: HoistedDependencies
    hoistedModulesDir: string
    hoistPattern?: string[]
    publicHoistPattern?: string[]
    include: IncludedDependencies
    linkedDependenciesByProjectId: Record<string, LinkedDependency[]>
    lockfileDir: string
    makePartialCurrentLockfile: boolean
    outdatedDependencies: {[pkgId: string]: string}
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
  depGraph = fromPairs(depNodes.map((depNode) => [depNode.depPath, depNode]))
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
  const newDepPaths = await linkNewPackages(
    filterLockfileByImporters(opts.currentLockfile, projectIds, {
      ...filterOpts,
      failOnMissingDependencies: false,
    }),
    newCurrentLockfile,
    depGraph,
    {
      force: opts.force,
      depsStateCache: opts.depsStateCache,
      lockfileDir: opts.lockfileDir,
      optional: opts.include.optionalDependencies,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      symlink: opts.symlink,
      skipped: opts.skipped,
      storeController: opts.storeController,
      virtualStoreDir: opts.virtualStoreDir,
    }
  )

  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_done',
  })

  if (opts.symlink) {
    await Promise.all(projects.map(async ({ id, manifest, modulesDir, rootDir }) => {
      const deps = opts.dependenciesByProjectId[id]
      await Promise.all([
        ...Object.entries(deps)
          .map(([rootAlias, depPath]) => ({ rootAlias, depGraphNode: depGraph[depPath] }))
          .filter(({ depGraphNode }) => depGraphNode)
          .map(async ({ rootAlias, depGraphNode }) => {
            const isDev = Boolean(manifest.devDependencies?.[depGraphNode.name])
            const isOptional = Boolean(manifest.optionalDependencies?.[depGraphNode.name])
            if (
              isDev && !opts.include.devDependencies ||
              isOptional && !opts.include.optionalDependencies ||
              !isDev && !isOptional && !opts.include.dependencies
            ) return
            if (
              (await symlinkDependency(depGraphNode.dir, modulesDir, rootAlias)).reused
            ) return

            rootLogger.debug({
              added: {
                dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
                id: depGraphNode.id,
                latest: opts.outdatedDependencies[depGraphNode.id],
                name: rootAlias,
                realName: depGraphNode.name,
                version: depGraphNode.version,
              },
              prefix: rootDir,
            })
          }),
        ...opts.linkedDependenciesByProjectId[id].map(async (linkedDependency) => {
          const depLocation = resolvePath(rootDir, linkedDependency.resolution.directory)
          return symlinkDirectRootDependency(depLocation, modulesDir, linkedDependency.alias, {
            fromDependenciesField: linkedDependency.dev && 'devDependencies' || linkedDependency.optional && 'optionalDependencies' || 'dependencies',
            linkedPackage: linkedDependency,
            prefix: rootDir,
          })
        }),
      ])
    }))
  }

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
    const projects = projectIds.reduce((acc, projectId) => {
      acc[projectId] = opts.wantedLockfile.importers[projectId]
      return acc
    }, opts.currentLockfile.importers)
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

  let newHoistedDependencies!: HoistedDependencies
  if ((opts.hoistPattern != null || opts.publicHoistPattern != null) && (newDepPaths.length > 0 || removedDepPaths.size > 0)) {
    // It is important to keep the skipped packages in the lockfile which will be saved as the "current lockfile".
    // pnpm is comparing the current lockfile to the wanted one and they should much.
    // But for hoisting, we need a version of the lockfile w/o the skipped packages, so we're making a copy.
    const hoistLockfile = {
      ...currentLockfile,
      packages: omit(Array.from(opts.skipped), currentLockfile.packages),
    }
    newHoistedDependencies = await hoist({
      lockfile: hoistLockfile,
      importerIds: projectIds,
      lockfileDir: opts.lockfileDir,
      privateHoistedModulesDir: opts.hoistedModulesDir,
      privateHoistPattern: opts.hoistPattern ?? [],
      publicHoistedModulesDir: opts.rootModulesDir,
      publicHoistPattern: opts.publicHoistPattern ?? [],
      virtualStoreDir: opts.virtualStoreDir,
    })
  } else {
    newHoistedDependencies = {}
  }

  return {
    currentLockfile,
    newDepPaths,
    newHoistedDependencies,
    removedDepPaths,
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
    optional: boolean
    lockfileDir: string
    sideEffectsCacheRead: boolean
    symlink: boolean
    skipped: Set<string>
    storeController: StoreController
    virtualStoreDir: string
  }
): Promise<string[]> {
  const wantedRelDepPaths = difference(Object.keys(wantedLockfile.packages ?? {}), Array.from(opts.skipped))

  let newDepPathsSet: Set<string>
  if (opts.force) {
    newDepPathsSet = new Set(
      wantedRelDepPaths
        // when installing a new package, not all the nodes are analyzed
        // just skip the ones that are in the lockfile but were not analyzed
        .filter((depPath) => depGraph[depPath])
    )
  } else {
    newDepPathsSet = await selectNewFromWantedDeps(wantedRelDepPaths, currentLockfile, depGraph)
  }

  statsLogger.debug({
    added: newDepPathsSet.size,
    prefix: opts.lockfileDir,
  })

  const existingWithUpdatedDeps = []
  if (!opts.force && (currentLockfile.packages != null) && (wantedLockfile.packages != null)) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const depPath of wantedRelDepPaths) {
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

  if (!newDepPathsSet.size && (existingWithUpdatedDeps.length === 0)) return []

  const newDepPaths = Array.from(newDepPathsSet)

  const newPkgs = props<string, DependenciesGraphNode>(newDepPaths, depGraph)

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
      lockfileDir: opts.lockfileDir,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
    }),
  ])

  return newDepPaths
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
          depNode.resolution['integrity'] === prevDep.resolution['integrity']
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
    lockfileDir: string
    sideEffectsCacheRead: boolean
  }
) {
  return Promise.all(
    depNodes.map(async (depNode) => {
      const filesResponse = await depNode.fetchingFiles()

      let targetEngine: string | undefined
      if (opts.sideEffectsCacheRead && filesResponse.sideEffects && !isEmpty(filesResponse.sideEffects)) {
        targetEngine = calcDepState(depNode.depPath, opts.depGraph, opts.depsStateCache)
      }
      const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
        filesResponse,
        force: opts.force,
        targetEngine,
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
        const childrenToLink = opts.optional
          ? children
          : Object.keys(children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = children[childAlias]
              }
              return nonOptionalChildren
            }, {})

        await Promise.all(
          Object.keys(childrenToLink)
            .map(async (childAlias) => {
              if (childrenToLink[childAlias].startsWith('link:')) {
                await limitLinking(async () => symlinkDependency(path.resolve(opts.lockfileDir, childrenToLink[childAlias].substr(5)), modules, childAlias))
                return
              }
              const pkg = depGraph[childrenToLink[childAlias]]
              if (!pkg || !pkg.installable && pkg.optional) return
              if (childAlias === name) {
                logger.warn({
                  message: `Cannot link dependency with name ${childAlias} to ${modules}. Dependency's name should differ from the parent's name.`,
                  prefix: opts.lockfileDir,
                })
                return
              }
              await limitLinking(async () => symlinkDependency(pkg.dir, modules, childAlias))
            })
        )
      })
  )
}
