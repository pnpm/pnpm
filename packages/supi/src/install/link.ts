import {
  ENGINE_NAME,
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
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
import matcher from '@pnpm/matcher'
import { prune } from '@pnpm/modules-cleaner'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import { DependenciesTree, LinkedDependency } from '@pnpm/resolve-dependencies'
import { StoreController } from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { ProjectManifest, Registries } from '@pnpm/types'
import fs = require('mz/fs')
import pLimit from 'p-limit'
import path = require('path')
import R = require('ramda')
import { depPathToRef } from './lockfile'
import resolvePeers, {
  DependenciesGraph,
  DependenciesGraphNode,
} from './resolvePeers'
import updateLockfile from './updateLockfile'

const brokenModulesLogger = logger('_broken_node_modules')

export {
  DependenciesGraph,
  DependenciesGraphNode,
}

export interface Project {
  binsDir: string,
  directNodeIdsByAlias: {[alias: string]: string},
  id: string,
  linkedDependencies: LinkedDependency[],
  manifest: ProjectManifest,
  modulesDir: string,
  pruneDirectDependencies: boolean,
  removePackages?: string[],
  rootDir: string,
  topParents: Array<{name: string, version: string}>,
}

export default async function linkPackages (
  projects: Project[],
  dependenciesTree: DependenciesTree,
  opts: {
    afterAllResolvedHook?: (lockfile: Lockfile) => Lockfile,
    currentLockfile: Lockfile,
    dryRun: boolean,
    force: boolean,
    hoistedAliases: {[depPath: string]: string[]},
    hoistedModulesDir: string,
    hoistPattern?: string[],
    include: IncludedDependencies,
    independentLeaves: boolean,
    lockfileDir: string,
    makePartialCurrentLockfile: boolean,
    outdatedDependencies: {[pkgId: string]: string},
    pruneStore: boolean,
    registries: Registries,
    sideEffectsCacheRead: boolean,
    skipped: Set<string>,
    storeController: StoreController,
    strictPeerDependencies: boolean,
    // This is only needed till lockfile v4
    updateLockfileMinorVersion: boolean,
    virtualStoreDir: string,
    wantedLockfile: Lockfile,
    wantedToBeSkippedPackageIds: Set<string>,
  },
): Promise<{
  currentLockfile: Lockfile,
  depGraph: DependenciesGraph,
  newDepPaths: string[],
  newHoistedAliases: {[depPath: string]: string[]},
  removedDepPaths: Set<string>,
  wantedLockfile: Lockfile,
}> {
  // TODO: decide what kind of logging should be here.
  // The `Creating dependency graph` is not good to report in all cases as
  // sometimes node_modules is alread up-to-date
  // logger.info(`Creating dependency graph`)
  const { depGraph, projectsDirectPathsByAlias } = resolvePeers({
    dependenciesTree,
    independentLeaves: opts.independentLeaves,
    lockfileDir: opts.lockfileDir,
    projects,
    strictPeerDependencies: opts.strictPeerDependencies,
    virtualStoreDir: opts.virtualStoreDir,
  })
  for (const { id } of projects) {
    for (const [alias, depPath] of R.toPairs(projectsDirectPathsByAlias[id])) {
      const depNode = depGraph[depPath]
      if (depNode.isPure) continue

      const projectSnapshot = opts.wantedLockfile.importers[id]
      const ref = depPathToRef(depPath, {
        alias,
        realName: depNode.name,
        registries: opts.registries,
        resolution: depNode.resolution,
      })
      if (projectSnapshot.dependencies?.[alias]) {
        projectSnapshot.dependencies[alias] = ref
      } else if (projectSnapshot.devDependencies?.[alias]) {
        projectSnapshot.devDependencies[alias] = ref
      } else if (projectSnapshot.optionalDependencies?.[alias]) {
        projectSnapshot.optionalDependencies[alias] = ref
      }
    }
  }
  const { newLockfile, pendingRequiresBuilds } = updateLockfile(depGraph, opts.wantedLockfile, opts.virtualStoreDir, opts.registries) // tslint:disable-line:prefer-const
  let newWantedLockfile = opts.afterAllResolvedHook
    ? opts.afterAllResolvedHook(newLockfile)
    : newLockfile

  let depNodes = R.values(depGraph).filter(({ depPath, packageId }) => {
    if (newWantedLockfile.packages?.[depPath] && !newWantedLockfile.packages[depPath].optional) {
      opts.skipped.delete(depPath)
      return true
    }
    if (opts.wantedToBeSkippedPackageIds.has(packageId)) {
      opts.skipped.add(depPath)
      return false
    }
    opts.skipped.delete(depPath)
    return true
  })
  if (!opts.include.dependencies) {
    depNodes = depNodes.filter(({ dev, optional }) => dev !== false || optional)
  }
  if (!opts.include.devDependencies) {
    depNodes = depNodes.filter(({ dev }) => dev !== true)
  }
  if (!opts.include.optionalDependencies) {
    depNodes = depNodes.filter(({ optional }) => !optional)
  }
  const removedDepPaths = await prune(projects, {
    currentLockfile: opts.currentLockfile,
    dryRun: opts.dryRun,
    hoistedAliases: opts.hoistedAliases,
    hoistedModulesDir: opts.hoistPattern && opts.hoistedModulesDir || undefined,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    pruneStore: opts.pruneStore,
    registries: opts.registries,
    skipped: opts.skipped,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    wantedLockfile: newWantedLockfile,
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
  const newCurrentLockfile = filterLockfileByImporters(newWantedLockfile, projectIds, {
    ...filterOpts,
    failOnMissingDependencies: true,
  })
  const newDepPaths = await linkNewPackages(
    filterLockfileByImporters(opts.currentLockfile, projectIds, {
      ...filterOpts,
      failOnMissingDependencies: false,
    }),
    newCurrentLockfile,
    depGraph,
    {
      dryRun: opts.dryRun,
      force: opts.force,
      lockfileDir: opts.lockfileDir,
      optional: opts.include.optionalDependencies,
      registries: opts.registries,
      storeController: opts.storeController,
      virtualStoreDir: opts.virtualStoreDir,
    },
  )

  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_done',
  })

  const rootDepsByDepPath = depNodes
    .filter(({ depth }) => depth === 0)
    .reduce((acc, depNode) => {
      acc[depNode.depPath] = depNode
      return acc
    }, {})

  await Promise.all(projects.map(({ id, manifest, modulesDir, rootDir }) => {
    const directPathsByAlias = projectsDirectPathsByAlias[id]
    return Promise.all(
      Object.keys(directPathsByAlias)
        .map((rootAlias) => ({ rootAlias, depGraphNode: rootDepsByDepPath[directPathsByAlias[rootAlias]] }))
        .filter(({ depGraphNode }) => depGraphNode)
        .map(async ({ rootAlias, depGraphNode }) => {
          if (
            !opts.dryRun &&
            (await symlinkDependency(depGraphNode.peripheralLocation, modulesDir, rootAlias)).reused
          ) return

          const isDev = manifest.devDependencies?.[depGraphNode.name]
          const isOptional = manifest.optionalDependencies?.[depGraphNode.name]
          rootLogger.debug({
            added: {
              dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
              id: depGraphNode.packageId,
              latest: opts.outdatedDependencies[depGraphNode.packageId],
              name: rootAlias,
              realName: depGraphNode.name,
              version: depGraphNode.version,
            },
            prefix: rootDir,
          })
        }),
    )
  }))

  if (opts.updateLockfileMinorVersion) {
    newWantedLockfile.lockfileVersion = LOCKFILE_VERSION
  }

  await Promise.all(pendingRequiresBuilds.map(async (depPath) => {
    const depNode = depGraph[depPath]
    if (!depNode.fetchingBundledManifest) {
      // This should never ever happen
      throw new Error(`Cannot create ${WANTED_LOCKFILE} because raw manifest (aka package.json) wasn't fetched for "${depPath}"`)
    }
    const filesResponse = await depNode.fetchingFiles()
    // The npm team suggests to always read the package.json for deciding whether the package has lifecycle scripts
    const pkgJson = await depNode.fetchingBundledManifest()
    depNode.requiresBuild = Boolean(
      pkgJson.scripts && (pkgJson.scripts.preinstall || pkgJson.scripts.install || pkgJson.scripts.postinstall) ||
      filesResponse.filesIndex['binding.gyp'] ||
        Object.keys(filesResponse.filesIndex).some((filename) => !!filename.match(/^[.]hooks[\\/]/)), // TODO: optimize this
    )

    // TODO: try to cover with unit test the case when entry is no longer available in lockfile
    // It is an edge that probably happens if the entry is removed during lockfile prune
    if (depNode.requiresBuild && newWantedLockfile.packages![depPath]) {
      newWantedLockfile.packages![depPath].requiresBuild = true
    }
  }))

  let currentLockfile: Lockfile
  const allImportersIncluded = R.equals(projectIds.sort(), Object.keys(newWantedLockfile.importers).sort())
  if (
    opts.makePartialCurrentLockfile ||
    !allImportersIncluded
  ) {
    const packages = opts.currentLockfile.packages || {}
    if (newWantedLockfile.packages) {
      for (const depPath in newWantedLockfile.packages) { // tslint:disable-line:forin
        if (depGraph[depPath]) {
          packages[depPath] = newWantedLockfile.packages[depPath]
        }
      }
    }
    const projects = projectIds.reduce((acc, projectId) => {
      acc[projectId] = newWantedLockfile.importers[projectId]
      return acc
    }, opts.currentLockfile.importers)
    currentLockfile = filterLockfileByImporters(
      {
        ...newWantedLockfile,
        importers: projects,
        packages,
      },
      Object.keys(projects), {
        ...filterOpts,
        failOnMissingDependencies: false,
      },
    )
  } else if (
    opts.include.dependencies &&
    opts.include.devDependencies &&
    opts.include.optionalDependencies &&
    opts.skipped.size === 0
  ) {
    currentLockfile = newWantedLockfile
  } else {
    currentLockfile = newCurrentLockfile
  }

  let newHoistedAliases: Record<string, string[]> = {}
  if (opts.hoistPattern && (newDepPaths.length > 0 || removedDepPaths.size > 0)) {
    newHoistedAliases = await hoist(matcher(opts.hoistPattern!), {
      getIndependentPackageLocation: opts.independentLeaves
        ? async (packageId: string, packageName: string) => {
          const { dir } = await opts.storeController.getPackageLocation(packageId, packageName, {
            lockfileDir: opts.lockfileDir,
            targetEngine: opts.sideEffectsCacheRead && ENGINE_NAME || undefined,
          })
          return dir
        }
        : undefined,
      lockfile: currentLockfile,
      lockfileDir: opts.lockfileDir,
      modulesDir: opts.hoistedModulesDir,
      registries: opts.registries,
      virtualStoreDir: opts.virtualStoreDir,
    })
  }

  if (!opts.dryRun) {
    await Promise.all(
      projects.map((project) =>
        Promise.all(project.linkedDependencies.map((linkedDependency) => {
          const depLocation = resolvePath(project.rootDir, linkedDependency.resolution.directory)
          return symlinkDirectRootDependency(depLocation, project.modulesDir, linkedDependency.alias, {
            fromDependenciesField: linkedDependency.dev && 'devDependencies' || linkedDependency.optional && 'optionalDependencies' || 'dependencies',
            linkedPackage: linkedDependency,
            prefix: project.rootDir,
          })
        })),
      ),
    )
  }

  return {
    currentLockfile,
    depGraph,
    newDepPaths,
    newHoistedAliases,
    removedDepPaths,
    wantedLockfile: newWantedLockfile,
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
    dryRun: boolean,
    force: boolean,
    optional: boolean,
    registries: Registries,
    lockfileDir: string,
    storeController: StoreController,
    virtualStoreDir: string,
  },
): Promise<string[]> {
  const wantedRelDepPaths = R.keys(wantedLockfile.packages)

  let newDepPathsSet: Set<string>
  if (opts.force) {
    newDepPathsSet = new Set(
      wantedRelDepPaths
        // when installing a new package, not all the nodes are analyzed
        // just skip the ones that are in the lockfile but were not analyzed
        .filter((depPath) => depGraph[depPath]),
    )
  } else {
    newDepPathsSet = await selectNewFromWantedDeps(wantedRelDepPaths, currentLockfile, depGraph, opts)
  }

  statsLogger.debug({
    added: newDepPathsSet.size,
    prefix: opts.lockfileDir,
  })

  const existingWithUpdatedDeps = []
  if (!opts.force && currentLockfile.packages && wantedLockfile.packages) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const depPath of wantedRelDepPaths) {
      if (currentLockfile.packages[depPath] &&
        (!R.equals(currentLockfile.packages[depPath].dependencies, wantedLockfile.packages[depPath].dependencies) ||
        !R.equals(currentLockfile.packages[depPath].optionalDependencies, wantedLockfile.packages[depPath].optionalDependencies))) {

        // TODO: come up with a test that triggers the usecase of depGraph[depPath] undefined
        // see related issue: https://github.com/pnpm/pnpm/issues/870
        if (depGraph[depPath] && !newDepPathsSet.has(depPath)) {
          existingWithUpdatedDeps.push(depGraph[depPath])
        }
      }
    }
  }

  if (!newDepPathsSet.size && !existingWithUpdatedDeps.length) return []

  const newDepPaths = Array.from(newDepPathsSet)

  if (opts.dryRun) return newDepPaths

  const newPkgs = R.props<string, DependenciesGraphNode>(newDepPaths, depGraph)

  await Promise.all([
    linkAllModules(newPkgs, depGraph, {
      lockfileDir: opts.lockfileDir,
      optional: opts.optional,
    }),
    linkAllModules(existingWithUpdatedDeps, depGraph, {
      lockfileDir: opts.lockfileDir,
      optional: opts.optional,
    }),
    linkAllPkgs(opts.storeController, newPkgs, opts),
  ])

  return newDepPaths
}

async function selectNewFromWantedDeps (
  wantedRelDepPaths: string[],
  currentLockfile: Lockfile,
  depGraph: DependenciesGraph,
  opts: {
    registries: Registries,
  },
) {
  const newDeps = new Set<string>()
  const prevRelDepPaths = new Set(R.keys(currentLockfile.packages))
  await Promise.all(
    wantedRelDepPaths.map(
      async (depPath: string) => {
        const depNode = depGraph[depPath]
        if (!depNode) return
        if (prevRelDepPaths.has(depPath)) {
          if (await fs.exists(depNode.peripheralLocation)) {
            return
          }
          brokenModulesLogger.debug({
            missing: depNode.peripheralLocation,
          })
        }
        newDeps.add(depPath)
      },
    ),
  )
  return newDeps
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: {
    force: boolean,
  },
) {
  return Promise.all(
    depNodes.map(async ({ fetchingFiles, independent, peripheralLocation }) => {
      const filesResponse = await fetchingFiles()

      return storeController.importPackage(peripheralLocation, {
        filesResponse,
        force: opts.force,
      })
    }),
  )
}

async function linkAllModules (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    lockfileDir: string,
    optional: boolean,
  },
) {
  return Promise.all(
    depNodes
      .filter(({ independent }) => !independent)
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
              const pkg = depGraph[childrenToLink[childAlias]]
              if (!pkg.installable && pkg.optional) return
              if (childAlias === name) {
                logger.warn({
                  message: `Cannot link dependency with name ${childAlias} to ${modules}. Dependency's name should differ from the parent's name.`,
                  prefix: opts.lockfileDir,
                })
                return
              }
              await limitLinking(() => symlinkDependency(pkg.peripheralLocation, modules, childAlias))
            }),
        )
      }),
  )
}
