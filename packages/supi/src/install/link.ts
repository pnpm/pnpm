import {
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  rootLogger,
  stageLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import filterLockfile, {
  filterLockfileByImporters,
} from '@pnpm/filter-lockfile'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import { Lockfile } from '@pnpm/lockfile-file'
import logger from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { DependenciesTree, LinkedDependency } from '@pnpm/resolve-dependencies'
import shamefullyFlattenGraph from '@pnpm/shamefully-flatten'
import { StoreController } from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { PackageJson, Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import fs = require('mz/fs')
import pFilter = require('p-filter')
import pLimit = require('p-limit')
import path = require('path')
import R = require('ramda')
import { absolutePathToRef } from './lockfile'
import resolvePeers, {
  DependenciesGraph,
  DependenciesGraphNode,
} from './resolvePeers'
import updateLockfile from './updateLockfile'

const brokenNodeModulesLogger = logger('_broken_node_modules')

export { DependenciesGraph }

export interface Importer {
  bin: string,
  directNodeIdsByAlias: {[alias: string]: string},
  hoistedAliases: {[depPath: string]: string[]},
  id: string,
  linkedDependencies: LinkedDependency[],
  modulesDir: string,
  pkg: PackageJson,
  prefix: string,
  pruneDirectDependencies: boolean,
  removePackages?: string[],
  shamefullyFlatten: boolean,
  topParents: Array<{name: string, version: string}>,
  usesExternalLockfile: boolean,
}

export default async function linkPackages (
  importers: Importer[],
  dependenciesTree: DependenciesTree,
  opts: {
    afterAllResolvedHook?: (lockfile: Lockfile) => Lockfile,
    force: boolean,
    dryRun: boolean,
    virtualStoreDir: string,
    wantedLockfile: Lockfile,
    currentLockfile: Lockfile,
    makePartialCurrentLockfile: boolean,
    pruneStore: boolean,
    registries: Registries,
    lockfileDirectory: string,
    skipped: Set<string>,
    storeController: StoreController,
    wantedToBeSkippedPackageIds: Set<string>,
    include: IncludedDependencies,
    independentLeaves: boolean,
    // This is only needed till lockfile v4
    updateLockfileMinorVersion: boolean,
    outdatedDependencies: {[pkgId: string]: string},
    strictPeerDependencies: boolean,
  },
): Promise<{
  currentLockfile: Lockfile,
  depGraph: DependenciesGraph,
  newDepPaths: string[],
  removedDepPaths: Set<string>,
  wantedLockfile: Lockfile,
}> {
  // TODO: decide what kind of logging should be here.
  // The `Creating dependency graph` is not good to report in all cases as
  // sometimes node_modules is alread up-to-date
  // logger.info(`Creating dependency graph`)
  const { depGraph, importersDirectAbsolutePathsByAlias } = resolvePeers({
    dependenciesTree,
    importers,
    independentLeaves: opts.independentLeaves,
    lockfileDirectory: opts.lockfileDirectory,
    strictPeerDependencies: opts.strictPeerDependencies,
    virtualStoreDir: opts.virtualStoreDir,
  })
  for (const importer of importers) {
    if (!importer.usesExternalLockfile) continue

    const directAbsolutePathsByAlias = importersDirectAbsolutePathsByAlias[importer.id]
    for (const alias of R.keys(directAbsolutePathsByAlias)) {
      const depPath = directAbsolutePathsByAlias[alias]

      const depNode = depGraph[depPath]
      if (depNode.isPure) continue

      const lockfileImporter = opts.wantedLockfile.importers[importer.id]
      const ref = absolutePathToRef(depPath, {
        alias,
        realName: depNode.name,
        registries: opts.registries,
        resolution: depNode.resolution,
      })
      if (lockfileImporter.dependencies && lockfileImporter.dependencies[alias]) {
        lockfileImporter.dependencies[alias] = ref
      } else if (lockfileImporter.devDependencies && lockfileImporter.devDependencies[alias]) {
        lockfileImporter.devDependencies[alias] = ref
      } else if (lockfileImporter.optionalDependencies && lockfileImporter.optionalDependencies[alias]) {
        lockfileImporter.optionalDependencies[alias] = ref
      }
    }
  }
  const { newLockfile, pendingRequiresBuilds } = updateLockfile(depGraph, opts.wantedLockfile, opts.virtualStoreDir, opts.registries) // tslint:disable-line:prefer-const
  let newWantedLockfile = opts.afterAllResolvedHook
    ? opts.afterAllResolvedHook(newLockfile)
    : newLockfile

  let depNodes = R.values(depGraph).filter((depNode) => {
    const relDepPath = dp.relative(opts.registries, depNode.name, depNode.absolutePath)
    if (newWantedLockfile.packages && newWantedLockfile.packages[relDepPath] && !newWantedLockfile.packages[relDepPath].optional) {
      opts.skipped.delete(relDepPath)
      return true
    }
    if (opts.wantedToBeSkippedPackageIds.has(depNode.id)) {
      opts.skipped.add(relDepPath)
      return false
    }
    opts.skipped.delete(relDepPath)
    return true
  })
  if (!opts.include.dependencies) {
    depNodes = depNodes.filter((depNode) => depNode.dev !== false || depNode.optional)
  }
  if (!opts.include.devDependencies) {
    depNodes = depNodes.filter((depNode) => depNode.dev !== true)
  }
  if (!opts.include.optionalDependencies) {
    depNodes = depNodes.filter((depNode) => !depNode.optional)
  }
  const filterOpts = {
    include: opts.include,
    registries: opts.registries,
    skipped: opts.skipped,
  }
  const removedDepPaths = await prune({
    dryRun: opts.dryRun,
    importers,
    lockfileDirectory: opts.lockfileDirectory,
    newLockfile: filterLockfile(newWantedLockfile, filterOpts),
    oldLockfile: opts.currentLockfile,
    pruneStore: opts.pruneStore,
    registries: opts.registries,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
  })

  stageLogger.debug({
    prefix: opts.lockfileDirectory,
    stage: 'importing_started',
  })

  const importerIds = importers.map((importer) => importer.id)
  const newCurrentLockfile = filterLockfileByImporters(newWantedLockfile, importerIds, {
    ...filterOpts,
    failOnMissingDependencies: true,
  })
  const newDepPaths = await linkNewPackages(
    filterLockfileByImporters(opts.currentLockfile, importerIds, {
      ...filterOpts,
      failOnMissingDependencies: false,
    }),
    newCurrentLockfile,
    depGraph,
    {
      dryRun: opts.dryRun,
      force: opts.force,
      lockfileDirectory: opts.lockfileDirectory,
      optional: opts.include.optionalDependencies,
      registries: opts.registries,
      storeController: opts.storeController,
      virtualStoreDir: opts.virtualStoreDir,
    },
  )

  stageLogger.debug({
    prefix: opts.lockfileDirectory,
    stage: 'importing_done',
  })

  const rootDepsByDepPath = depNodes
    .filter((depNode) => depNode.depth === 0)
    .reduce((acc, depNode) => {
      acc[depNode.absolutePath] = depNode
      return acc
    }, {}) as {[absolutePath: string]: DependenciesGraphNode}

  await Promise.all(importers.map((importer) => {
    const directAbsolutePathsByAlias = importersDirectAbsolutePathsByAlias[importer.id]
    const { modulesDir, pkg, prefix } = importer
    return Promise.all(
      R.keys(directAbsolutePathsByAlias)
        .map((rootAlias) => ({ rootAlias, depGraphNode: rootDepsByDepPath[directAbsolutePathsByAlias[rootAlias]] }))
        .filter(({ depGraphNode }) => depGraphNode)
        .map(async ({ rootAlias, depGraphNode }) => {
          if (
            !opts.dryRun &&
            (await symlinkDependency(depGraphNode.peripheralLocation, modulesDir, rootAlias)).reused
          ) return

          const isDev = pkg.devDependencies && pkg.devDependencies[depGraphNode.name]
          const isOptional = pkg.optionalDependencies && pkg.optionalDependencies[depGraphNode.name]
          rootLogger.debug({
            added: {
              dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
              id: depGraphNode.id,
              latest: opts.outdatedDependencies[depGraphNode.id],
              name: rootAlias,
              realName: depGraphNode.name,
              version: depGraphNode.version,
            },
            prefix,
          })
        }),
    )
  }))

  if (opts.updateLockfileMinorVersion) {
    newWantedLockfile.lockfileVersion = LOCKFILE_VERSION
  }

  await Promise.all(pendingRequiresBuilds.map(async (pendingRequiresBuild) => {
    const depNode = depGraph[pendingRequiresBuild.absoluteDepPath]
    if (!depNode.fetchingRawManifest) {
      // This should never ever happen
      throw new Error(`Cannot create ${WANTED_LOCKFILE} because raw manifest (aka package.json) wasn't fetched for "${pendingRequiresBuild.absoluteDepPath}"`)
    }
    const filesResponse = await depNode.fetchingFiles
    // The npm team suggests to always read the package.json for deciding whether the package has lifecycle scripts
    const pkgJson = await depNode.fetchingRawManifest
    depNode.requiresBuild = Boolean(
      pkgJson.scripts && (pkgJson.scripts.preinstall || pkgJson.scripts.install || pkgJson.scripts.postinstall) ||
      filesResponse.filenames.indexOf('binding.gyp') !== -1 ||
        filesResponse.filenames.some((filename) => !!filename.match(/^[.]hooks[\\/]/)), // TODO: optimize this
    )

    // TODO: try to cover with unit test the case when entry is no longer available in lockfile
    // It is an edge that probably happens if the entry is removed during lockfile prune
    if (depNode.requiresBuild && newWantedLockfile.packages![pendingRequiresBuild.relativeDepPath]) {
      newWantedLockfile.packages![pendingRequiresBuild.relativeDepPath].requiresBuild = true
    }
  }))

  let currentLockfile: Lockfile
  const allImportersIncluded = R.equals(importerIds.sort(), Object.keys(newWantedLockfile.importers).sort())
  if (
    opts.makePartialCurrentLockfile ||
    !allImportersIncluded
  ) {
    const filteredCurrentLockfile = allImportersIncluded
      ? opts.currentLockfile
      : filterLockfileByImporters(
        opts.currentLockfile,
        Object.keys(newWantedLockfile.importers)
          .filter((importerId) => importerIds.indexOf(importerId) === -1 && opts.currentLockfile.importers[importerId]),
        {
          ...filterOpts,
          failOnMissingDependencies: false,
        },
      )
    const packages = filteredCurrentLockfile.packages || {}
    if (newWantedLockfile.packages) {
      for (const relDepPath in newWantedLockfile.packages) { // tslint:disable-line:forin
        const depPath = dp.resolve(opts.registries, relDepPath)
        if (depGraph[depPath]) {
          packages[relDepPath] = newWantedLockfile.packages[relDepPath]
        }
      }
    }
    const importers = importerIds.reduce((acc, importerId) => {
      acc[importerId] = newWantedLockfile.importers[importerId]
      return acc
    }, {})
    currentLockfile = { ...newWantedLockfile, packages, importers }
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

  // Important: shamefullyFlattenGraph changes depGraph, so keep this at the end, right before linkBins
  if (newDepPaths.length > 0 || removedDepPaths.size > 0) {
    await Promise.all(
      importers.filter((importer) => importer.shamefullyFlatten)
        .map(async (importer) => {
          importer.hoistedAliases = await shamefullyFlattenGraph(
            depNodes.map((depNode) => ({
              absolutePath: depNode.absolutePath,
              children: depNode.children,
              depth: depNode.depth,
              location: depNode.independent ? depNode.centralLocation : depNode.peripheralLocation,
              name: depNode.name,
            })),
            currentLockfile.importers[importer.id].specifiers,
            {
              dryRun: opts.dryRun,
              modulesDir: importer.modulesDir,
            },
          )
        }),
    )
  }

  if (!opts.dryRun) {
    await Promise.all(
      importers.map((importer) =>
        Promise.all(importer.linkedDependencies.map((linkedDependency) => {
          const depLocation = resolvePath(importer.prefix, linkedDependency.resolution.directory)
          return symlinkDirectRootDependency(depLocation, importer.modulesDir, linkedDependency.alias, {
            fromDependenciesField: linkedDependency.dev && 'devDependencies' || linkedDependency.optional && 'optionalDependencies' || 'dependencies',
            linkedPackage: linkedDependency,
            prefix: importer.prefix,
          })
        })),
      ),
    )

    await Promise.all(importers.map(linkBinsOfImporter))
  }

  return {
    currentLockfile,
    depGraph,
    newDepPaths,
    removedDepPaths,
    wantedLockfile: newWantedLockfile,
  }
}

function linkBinsOfImporter ({ modulesDir, bin, prefix }: Importer) {
  const warn = (message: string) => logger.warn({ message, prefix })
  return linkBins(modulesDir, bin, { warn })
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
    lockfileDirectory: string,
    storeController: StoreController,
    virtualStoreDir: string,
  },
): Promise<string[]> {
  const wantedRelDepPaths = R.keys(wantedLockfile.packages)

  const newDepPathsSet = await (async () => {
    if (opts.force) {
      return new Set(
        wantedRelDepPaths
          .map((relDepPath) => dp.resolve(opts.registries, relDepPath))
          // when installing a new package, not all the nodes are analyzed
          // just skip the ones that are in the lockfile but were not analyzed
          .filter((depPath) => depGraph[depPath]),
      )
    }
    const s = new Set()
    const prevRelDepPaths = new Set(R.keys(currentLockfile.packages))
    await Promise.all(
      wantedRelDepPaths.map(
      async (wantedRelDepPath: string) => {
        const depPath = dp.resolve(opts.registries, wantedRelDepPath)
        const depNode = depGraph[depPath]
        if (!depNode) return
        if (prevRelDepPaths.has(wantedRelDepPath) && !depNode.independent) {
          if (await fs.exists(depNode.peripheralLocation)) {
            return
          }
          brokenNodeModulesLogger.debug({
            missing: depNode.peripheralLocation,
          })
        }
        s.add(depPath)
      })
    )
    return s
  })()
  statsLogger.debug({
    added: newDepPathsSet.size,
    prefix: opts.lockfileDirectory,
  })

  const existingWithUpdatedDeps = []
  if (!opts.force && currentLockfile.packages && wantedLockfile.packages) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const relDepPath of wantedRelDepPaths) {
      if (currentLockfile.packages[relDepPath] &&
        (!R.equals(currentLockfile.packages[relDepPath].dependencies, wantedLockfile.packages[relDepPath].dependencies) ||
        !R.equals(currentLockfile.packages[relDepPath].optionalDependencies, wantedLockfile.packages[relDepPath].optionalDependencies))) {
        const depPath = dp.resolve(opts.registries, relDepPath)

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
      lockfileDirectory: opts.lockfileDirectory,
      optional: opts.optional,
    }),
    linkAllModules(existingWithUpdatedDeps, depGraph, {
      lockfileDirectory: opts.lockfileDirectory,
      optional: opts.optional,
    }),
    linkAllPkgs(opts.storeController, newPkgs, opts),
  ])

  await linkAllBins(newPkgs, depGraph, {
    optional: opts.optional,
    warn: (message: string) => logger.warn({ message, prefix: opts.lockfileDirectory }),
  })

  return newDepPaths
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
    depNodes.map(async (depNode) => {
      const filesResponse = await depNode.fetchingFiles

      if (depNode.independent) return
      return storeController.importPackage(depNode.centralLocation, depNode.peripheralLocation, {
        filesResponse,
        force: opts.force,
      })
    }),
  )
}

async function linkAllBins (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    optional: boolean,
    warn: (message: string) => void,
  },
) {
  return Promise.all(
    depNodes.map((depNode) => limitLinking(async () => {
      const childrenToLink = opts.optional
          ? depNode.children
          : R.keys(depNode.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!depNode.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = depNode.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

      const pkgs = await Promise.all(
        R.keys(childrenToLink)
          .filter((alias) => depGraph[childrenToLink[alias]].hasBin && depGraph[childrenToLink[alias]].installable)
          .map(async (alias) => {
            const dep = depGraph[childrenToLink[alias]]
            return {
              location: dep.peripheralLocation,
              manifest: (await dep.fetchingRawManifest) || await readPackageFromDir(dep.peripheralLocation),
            }
          }),
      )

      const binPath = path.join(depNode.peripheralLocation, 'node_modules', '.bin')
      await linkBinsOfPackages(pkgs, binPath, { warn: opts.warn })

      // link also the bundled dependencies` bins
      if (depNode.hasBundledDependencies) {
        const bundledModules = path.join(depNode.peripheralLocation, 'node_modules')
        await linkBins(bundledModules, binPath, { warn: opts.warn })
      }
    })),
  )
}

async function linkAllModules (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    lockfileDirectory: string,
    optional: boolean,
  },
) {
  return Promise.all(
    depNodes
      .filter((depNode) => !depNode.independent)
      .map((depNode) => limitLinking(async () => {
        const childrenToLink = opts.optional
          ? depNode.children
          : R.keys(depNode.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!depNode.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = depNode.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

        await Promise.all(
          R.keys(childrenToLink)
            .map(async (alias) => {
              const pkg = depGraph[childrenToLink[alias]]
              if (!pkg.installable && pkg.optional) return
              if (alias === depNode.name) {
                logger.warn({
                  message: `Cannot link dependency with name ${alias} to ${depNode.modules}. Dependency's name should differ from the parent's name.`,
                  prefix: opts.lockfileDirectory,
                })
                return
              }
              await symlinkDependency(pkg.peripheralLocation, depNode.modules, alias)
            }),
        )
      })),
  )
}
