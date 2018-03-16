import {
  LogBase,
  streamParser,
} from '@pnpm/logger'
import {
  getCacheByEngine,
  PackageFilesResponse,
} from '@pnpm/package-requester'
import {PackageJson} from '@pnpm/types'
import dp = require('dependency-path')
import pLimit = require('p-limit')
import {StoreController} from 'package-store'
import path = require('path')
import {
  PackageSnapshot,
  readWanted,
  Shrinkwrap,
  writeCurrentOnly as writeCurrentShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import readPkgCB = require('read-package-json')
import {
  LAYOUT_VERSION,
  save as saveModules,
} from 'supi/lib/fs/modulesController'
import realNodeModulesDir from 'supi/lib/fs/realNodeModulesDir'
import getPkgInfoFromShr from 'supi/lib/getPkgInfoFromShr'
import {npmRunScript} from 'supi/lib/install/postInstall'
import linkBins, {linkPkgBins} from 'supi/lib/link/linkBins' // TODO: move to separate package
import {
  rootLogger,
  stageLogger,
  summaryLogger,
} from 'supi/lib/loggers'
import logStatus from 'supi/lib/logging/logInstallStatus'
import symlinkDir = require('symlink-dir')
import promisify = require('util.promisify')
import {ENGINE_NAME} from './constants'
import depSnapshotToResolution from './depSnapshotToResolution'
import runDependenciesScripts from './runDependenciesScripts'

const readPkg = promisify(readPkgCB)

export type ReporterFunction = (logObj: LogBase) => void

export default async (
  opts: {
    childConcurrency?: number,
    development: boolean,
    optional: boolean,
    prefix: string,
    production: boolean,
    ignoreScripts: boolean,
    independentLeaves: boolean,
    storeController: StoreController,
    verifyStoreIntegrity: boolean,
    sideEffectsCache: boolean,
    sideEffectsCacheReadonly: boolean,
    force: boolean,
    storePath: string,
    rawNpmConfig: object,
    unsafePerm: boolean,
    userAgent: string,
    reporter?: ReporterFunction,
    packageManager: {
      name: string,
      version: string,
    },
  },
) => {
  const reporter = opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  if (typeof opts.prefix !== 'string') {
    throw new TypeError('opts.prefix should be a string')
  }

  const wantedShrinkwrap = await readWanted(opts.prefix, {ignoreIncompatible: false})

  if (!wantedShrinkwrap) {
    throw new Error('Headless installation can be done only with a shrinkwrap.yaml file')
  }

  const pkg = await readPkg(path.join(opts.prefix, 'package.json')) as PackageJson

  const scripts = !opts.ignoreScripts && pkg.scripts || {}

  const scriptsOpts = {
    modulesDir: await realNodeModulesDir(opts.prefix),
    pkgId: opts.prefix,
    rawNpmConfig: opts.rawNpmConfig,
    root: opts.prefix,
    stdio: 'inherit',
    unsafePerm: opts.unsafePerm || false,
  }

  if (scripts.preinstall) {
    await npmRunScript('preinstall', pkg, scriptsOpts)
  }

  const filterOpts = {
    noDev: !opts.development,
    noOptional: !opts.optional,
    noProd: !opts.production,
  }
  const filteredShrinkwrap = filterShrinkwrap(wantedShrinkwrap, filterOpts)

  stageLogger.debug('importing_started')
  const depGraph = await shrinkwrapToDepGraph(filteredShrinkwrap, opts)

  await Promise.all([
    linkAllModules(depGraph, {optional: opts.optional}),
    linkAllPkgs(opts.storeController, R.values(depGraph), opts),
  ])
  stageLogger.debug('importing_done')

  await linkAllBins(depGraph, {optional: opts.optional})

  const nodeModules = path.join(opts.prefix, 'node_modules')
  const bin = path.join(opts.prefix, 'node_modules', '.bin')
  await linkRootPackages(filteredShrinkwrap, depGraph, nodeModules)
  await linkBins(nodeModules, bin)

  await writeCurrentShrinkwrapOnly(opts.prefix, filteredShrinkwrap)
  await saveModules(path.join(opts.prefix, 'node_modules'), {
    hoistedAliases: {},
    independentLeaves: !!opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: [], // TODO: populate this array when runnig with --ignore-scripts
    shamefullyFlatten: false,
    skipped: [],
    store: opts.storePath,
  })

  if (!opts.ignoreScripts) {
    await runDependenciesScripts(depGraph, opts)
  }

  // waiting till package requests are finished
  await Promise.all(R.values(depGraph).map((depNode) => depNode.finishing))

  summaryLogger.info(undefined)

  await opts.storeController.close()

  if (scripts.install) {
    await npmRunScript('install', pkg, scriptsOpts)
  }
  if (scripts.postinstall) {
    await npmRunScript('postinstall', pkg, scriptsOpts)
  }
  if (scripts.prepublish) {
    await npmRunScript('prepublish', pkg, scriptsOpts)
  }
  if (scripts.prepare) {
    await npmRunScript('prepare', pkg, scriptsOpts)
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function linkRootPackages (
  shr: Shrinkwrap,
  depGraph: DepGraphNodesByDepPath,
  baseNodeModules: string,
) {
  const allDeps = Object.assign({}, shr.devDependencies, shr.dependencies, shr.optionalDependencies)
  return R.keys(allDeps)
    .map(async (alias) => {
      const depPath = dp.refToAbsolute(allDeps[alias], alias, shr.registry)
      const depNode = depGraph[depPath]
      await symlinkDependencyTo(alias, depNode, baseNodeModules)
      const isDev = shr.devDependencies && shr.devDependencies[alias]
      const isOptional = shr.optionalDependencies && shr.optionalDependencies[alias]

      const relDepPath = dp.refToRelative(allDeps[alias], alias)
      const depSnapshot = shr.packages && shr.packages[relDepPath]
      if (!depSnapshot) return // this won't ever happen. Just making typescript happy
      const pkgId = depSnapshot.id || depPath
      const pkgInfo = getPkgInfoFromShr(relDepPath, depSnapshot)
      rootLogger.info({
        added: {
          dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
          id: pkgId,
          // latest: opts.outdatedPkgs[pkg.id],
          name: alias,
          realName: pkgInfo.name,
          version: pkgInfo.version,
        },
      })
      logStatus({
        pkgId,
        status: 'installed',
      })
    })
}

async function shrinkwrapToDepGraph (
  shr: Shrinkwrap,
  opts: {
    force: boolean,
    independentLeaves: boolean,
    storeController: StoreController,
    storePath: string,
    prefix: string,
    verifyStoreIntegrity: boolean,
  },
) {
  const nodeModules = path.join(opts.prefix, 'node_modules')
  const graph: DepGraphNodesByDepPath = {}
  if (shr.packages) {
    for (const relDepPath of R.keys(shr.packages)) {
      const depPath = dp.resolve(shr.registry, relDepPath)
      const depSnapshot = shr.packages[relDepPath]
      const independent = opts.independentLeaves && R.isEmpty(depSnapshot.dependencies) && R.isEmpty(depSnapshot.optionalDependencies)
      const resolution = depSnapshotToResolution(relDepPath, depSnapshot, shr.registry)
      // TODO: optimize. This info can be already returned by depSnapshotToResolution()
      const pkgName = depSnapshot.name || dp.parse(relDepPath)['name'] // tslint:disable-line
      const pkgId = depSnapshot.id || depPath
      const fetchResponse = opts.storeController.fetchPackage({
        force: false,
        pkgId,
        prefix: opts.prefix,
        resolution,
        verifyStoreIntegrity: opts.verifyStoreIntegrity,
      })
      const cacheByEngine = opts.force ? new Map() : await getCacheByEngine(opts.storePath, pkgId)
      const cache = cacheByEngine[ENGINE_NAME]
      const centralLocation = cache || path.join(fetchResponse.inStoreLocation, 'node_modules', pkgName)

      // TODO: make this work with local deps. Local deps have IDs that can be converted to location, only via `.${pkgIdToFilename(node.pkg.id)}`
      const modules = path.join(nodeModules, `.${depPath}`, 'node_modules')
      const peripheralLocation = !independent
        ? path.join(modules, pkgName)
        : centralLocation
      graph[depPath] = {
        centralLocation,
        children: getChildren(depSnapshot, shr.registry),
        fetchingFiles: fetchResponse.fetchingFiles,
        finishing: fetchResponse.finishing,
        hasBundledDependencies: !!depSnapshot.bundledDependencies,
        independent,
        isBuilt: !!cache,
        modules,
        optional: !!depSnapshot.optional,
        optionalDependencies: new Set(R.keys(depSnapshot.optionalDependencies)),
        peripheralLocation,
        pkgId,
      }
    }
  }
  return graph
}

function getChildren (depSnapshot: PackageSnapshot, registry: string) {
  const allDeps = Object.assign({}, depSnapshot.dependencies, depSnapshot.optionalDependencies)
  return R.keys(allDeps)
    .reduce((acc, alias) => {
      acc[alias] = dp.refToAbsolute(allDeps[alias], alias, registry)
      return acc
    }, {})
}

export interface DepGraphNode {
  hasBundledDependencies: boolean,
  centralLocation: string,
  modules: string,
  fetchingFiles: Promise<PackageFilesResponse>,
  finishing: Promise<void>,
  peripheralLocation: string,
  children: {[alias: string]: string},
  // an independent package is a package that
  // has neither regular nor peer dependencies
  independent: boolean,
  optionalDependencies: Set<string>,
  optional: boolean,
  pkgId: string, // TODO: this option is currently only needed when running postinstall scripts but even there it should be not used
  isBuilt: boolean,
}

export interface DepGraphNodesByDepPath {
  [depPath: string]: DepGraphNode
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DepGraphNode[],
  opts: {
    force: boolean,
    sideEffectsCache: boolean,
  },
) {
  return Promise.all(
    depNodes.map(async (depNode) => {
      const filesResponse = await depNode.fetchingFiles

      // if (depNode.independent) return
      return storeController.importPackage(depNode.centralLocation, depNode.peripheralLocation, {
        filesResponse,
        force: opts.force,
      })
    }),
  )
}

async function linkAllBins (
  depGraph: DepGraphNodesByDepPath,
  opts: {
    optional: boolean,
  },
) {
  return Promise.all(
    R.values(depGraph).map((depNode) => limitLinking(async () => {
      const binPath = path.join(depNode.peripheralLocation, 'node_modules', '.bin')

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
          // .filter((alias) => depGraph[childrenToLink[alias]].installable)
          .map((alias) => path.join(depNode.modules, alias))
          .map((target) => linkPkgBins(target, binPath)),
      )

      // link also the bundled dependencies` bins
      if (depNode.hasBundledDependencies) {
        const bundledModules = path.join(depNode.peripheralLocation, 'node_modules')
        await linkBins(bundledModules, binPath)
      }
    })),
  )
}

async function linkAllModules (
  depGraph: DepGraphNodesByDepPath,
  opts: {
    optional: boolean,
  },
) {
  return Promise.all(
    R.values(depGraph)
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
              // if (!pkg.installable) return
              await symlinkDependencyTo(alias, pkg, depNode.modules)
            }),
        )
      })),
  )
}

function symlinkDependencyTo (alias: string, depNode: DepGraphNode, dest: string) {
  dest = path.join(dest, alias)
  return symlinkDir(depNode.peripheralLocation, dest)
}

// TODO: move this to separate package
// the version of the function which is in supi also accepts `opts.skip`
// headless will never skip anything
function filterShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    noDev: boolean,
    noOptional: boolean,
    noProd: boolean,
  },
): Shrinkwrap {
  let pairs = R.toPairs(shr.packages || {}) as Array<[string, PackageSnapshot]>
  if (opts.noProd) {
    pairs = pairs.filter((pair) => pair[1].dev !== false || pair[1].optional)
  }
  if (opts.noDev) {
    pairs = pairs.filter((pair) => pair[1].dev !== true)
  }
  if (opts.noOptional) {
    pairs = pairs.filter((pair) => !pair[1].optional)
  }
  return {
    dependencies: opts.noProd ? {} : shr.dependencies || {},
    devDependencies: opts.noDev ? {} : shr.devDependencies || {},
    optionalDependencies: opts.noOptional ? {} : shr.optionalDependencies || {},
    packages: R.fromPairs(pairs),
    registry: shr.registry,
    shrinkwrapVersion: shr.shrinkwrapVersion,
    specifiers: shr.specifiers,
  } as Shrinkwrap
}
