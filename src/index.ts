import runLifecycleHooks from '@pnpm/lifecycle'
import linkBins, {linkPackageBins} from '@pnpm/link-bins'
import {
  LogBase,
  streamParser,
} from '@pnpm/logger'
import {
  read as readModulesYaml,
  write as writeModulesYaml,
} from '@pnpm/modules-yaml'
import {
  getCacheByEngine,
  PackageFilesResponse,
} from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {PackageJson} from '@pnpm/types'
import dp = require('dependency-path')
import pLimit = require('p-limit')
import {StoreController} from 'package-store'
import path = require('path')
import {
  nameVerFromPkgSnapshot,
  PackageSnapshot,
  pkgSnapshotToResolution,
  readCurrent,
  readWanted,
  satisfiesPackageJson,
  Shrinkwrap,
  writeCurrentOnly as writeCurrentShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import readPkgCB = require('read-package-json')
import removeOrphanPkgs from 'supi/lib/api/removeOrphanPkgs'
import realNodeModulesDir from 'supi/lib/fs/realNodeModulesDir'
import {
  packageJsonLogger,
  rootLogger,
  stageLogger,
  statsLogger,
  summaryLogger,
} from 'supi/lib/loggers'
import symlinkDir = require('symlink-dir')
import promisify = require('util.promisify')
import {
  ENGINE_NAME,
  LAYOUT_VERSION,
} from './constants'
import runDependenciesScripts from './runDependenciesScripts'

const readPkg = promisify(readPkgCB)

export type ReporterFunction = (logObj: LogBase) => void

export interface HeadlessOptions {
  childConcurrency?: number,
  currentShrinkwrap?: Shrinkwrap,
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
  store: string,
  rawNpmConfig: object,
  unsafePerm: boolean,
  userAgent: string,
  reporter?: ReporterFunction,
  packageJson?: PackageJson,
  packageManager: {
    name: string,
    version: string,
  },
  wantedShrinkwrap?: Shrinkwrap,
}

export default async (opts: HeadlessOptions) => {
  const reporter = opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  if (typeof opts.prefix !== 'string') {
    throw new TypeError('opts.prefix should be a string')
  }

  const wantedShrinkwrap = opts.wantedShrinkwrap || await readWanted(opts.prefix, {ignoreIncompatible: false})

  if (!wantedShrinkwrap) {
    throw new Error('Headless installation requires a shrinkwrap.yaml file')
  }

  const currentShrinkwrap = opts.currentShrinkwrap || await readCurrent(opts.prefix, {ignoreIncompatible: false})
  const modules = await readModulesYaml(path.join(opts.prefix, 'node_modules'))

  const pkg = opts.packageJson || await readPkg(path.join(opts.prefix, 'package.json')) as PackageJson

  if (!satisfiesPackageJson(wantedShrinkwrap, pkg)) {
    throw new Error('Cannot run headless installation because shrinkwrap.yaml is not up-to-date with package.json')
  }

  packageJsonLogger.debug({ initial: pkg })

  const scripts = !opts.ignoreScripts && pkg.scripts || {}

  const nodeModules = await realNodeModulesDir(opts.prefix)
  const bin = path.join(nodeModules, '.bin')

  const scriptsOpts = {
    depPath: opts.prefix,
    pkgRoot: opts.prefix,
    rawNpmConfig: opts.rawNpmConfig,
    rootNodeModulesDir: nodeModules,
    stdio: 'inherit',
    unsafePerm: opts.unsafePerm || false,
  }

  if (scripts.preinstall) {
    await runLifecycleHooks('preinstall', pkg, scriptsOpts)
  }

  if (currentShrinkwrap) {
    await removeOrphanPkgs({
      bin,
      dryRun: false,
      hoistedAliases: modules && modules.hoistedAliases || {},
      newShrinkwrap: wantedShrinkwrap,
      oldShrinkwrap: currentShrinkwrap,
      prefix: opts.prefix,
      shamefullyFlatten: false,
      storeController: opts.storeController,
    })
  } else {
    statsLogger.debug({
      prefix: opts.prefix,
      removed: 0,
    })
  }

  const filterOpts = {
    noDev: !opts.development,
    noOptional: !opts.optional,
    noProd: !opts.production,
  }
  const filteredShrinkwrap = filterShrinkwrap(wantedShrinkwrap, filterOpts)

  stageLogger.debug('importing_started')
  const res = await shrinkwrapToDepGraph(filteredShrinkwrap, opts.force ? null : currentShrinkwrap, opts)
  const depGraph = res.graph

  statsLogger.debug({
    added: Object.keys(depGraph).length,
    prefix: opts.prefix,
  })

  await Promise.all([
    linkAllModules(depGraph, {optional: opts.optional}),
    linkAllPkgs(opts.storeController, R.values(depGraph), opts),
  ])
  stageLogger.debug('importing_done')

  await linkAllBins(depGraph, {optional: opts.optional})

  await linkRootPackages(filteredShrinkwrap, depGraph, res.rootDependencies, nodeModules)
  await linkBins(nodeModules, bin)

  await writeCurrentShrinkwrapOnly(opts.prefix, filteredShrinkwrap)
  await writeModulesYaml(path.join(opts.prefix, 'node_modules'), {
    hoistedAliases: {},
    independentLeaves: !!opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: [], // TODO: populate this array when runnig with --ignore-scripts
    shamefullyFlatten: false,
    skipped: [],
    store: opts.store,
  })

  if (!opts.ignoreScripts) {
    await runDependenciesScripts(depGraph, opts)
  }

  // waiting till package requests are finished
  await Promise.all(R.values(depGraph).map((depNode) => depNode.finishing))

  summaryLogger.info(undefined)

  await opts.storeController.close()

  if (scripts.install) {
    await runLifecycleHooks('install', pkg, scriptsOpts)
  }
  if (scripts.postinstall) {
    await runLifecycleHooks('postinstall', pkg, scriptsOpts)
  }
  if (scripts.prepublish) {
    await runLifecycleHooks('prepublish', pkg, scriptsOpts)
  }
  if (scripts.prepare) {
    await runLifecycleHooks('prepare', pkg, scriptsOpts)
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function linkRootPackages (
  shr: Shrinkwrap,
  depGraph: DepGraphNodesByDepPath,
  rootDependencies: {[alias: string]: string},
  baseNodeModules: string,
) {
  const allDeps = Object.assign({}, shr.devDependencies, shr.dependencies, shr.optionalDependencies)
  return Promise.all(
    R.keys(allDeps)
      .map(async (alias) => {
        const depPath = dp.refToAbsolute(allDeps[alias], alias, shr.registry)
        const peripheralLocation = rootDependencies[alias]
        // Skipping linked packages
        if (!peripheralLocation) {
          return
        }
        if ((await symlinkDependencyTo(alias, peripheralLocation, baseNodeModules)).reused) {
          return
        }
        const isDev = shr.devDependencies && shr.devDependencies[alias]
        const isOptional = shr.optionalDependencies && shr.optionalDependencies[alias]

        const relDepPath = dp.refToRelative(allDeps[alias], alias)
        const pkgSnapshot = shr.packages && shr.packages[relDepPath]
        if (!pkgSnapshot) return // this won't ever happen. Just making typescript happy
        const pkgId = pkgSnapshot.id || depPath
        const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
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
      }),
  )
}

async function shrinkwrapToDepGraph (
  shr: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap | null,
  opts: {
    force: boolean,
    independentLeaves: boolean,
    storeController: StoreController,
    store: string,
    prefix: string,
    verifyStoreIntegrity: boolean,
  },
) {
  const nodeModules = path.join(opts.prefix, 'node_modules')
  const currentPackages = currentShrinkwrap && currentShrinkwrap.packages || {}
  const graph: DepGraphNodesByDepPath = {}
  let rootDependencies: {[alias: string]: string} = {}
  if (shr.packages) {
    const pkgSnapshotByDepPath = {}
    for (const relDepPath of R.keys(shr.packages)) {
      if (currentPackages[relDepPath] && R.equals(currentPackages[relDepPath].dependencies, shr.packages[relDepPath].dependencies) &&
        R.equals(currentPackages[relDepPath].optionalDependencies, shr.packages[relDepPath].optionalDependencies)) {
          continue
        }
      const depPath = dp.resolve(shr.registry, relDepPath)
      const pkgSnapshot = shr.packages[relDepPath]
      pkgSnapshotByDepPath[depPath] = pkgSnapshot
      const independent = opts.independentLeaves && pkgIsIndependent(pkgSnapshot)
      const resolution = pkgSnapshotToResolution(relDepPath, pkgSnapshot, shr.registry)
      // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
      const pkgName = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot).name
      const pkgId = pkgSnapshot.id || depPath
      let fetchResponse = opts.storeController.fetchPackage({
        force: false,
        pkgId,
        prefix: opts.prefix,
        resolution,
        verifyStoreIntegrity: opts.verifyStoreIntegrity,
      })
      if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
      const cache = !opts.force && await getCache(opts.store, pkgId)
      const centralLocation = cache || path.join(fetchResponse.inStoreLocation, 'node_modules', pkgName)

      // NOTE: This code will not convert the depPath with peer deps correctly
      // Unfortunately, there is currently no way to tell if the last dir in the path is originally there or added to separate
      // the diferent peer dependency sets
      const modules = path.join(nodeModules, `.${pkgIdToFilename(depPath)}`, 'node_modules')
      const peripheralLocation = !independent
        ? path.join(modules, pkgName)
        : centralLocation
      graph[depPath] = {
        centralLocation,
        children: {},
        fetchingFiles: fetchResponse.fetchingFiles,
        finishing: fetchResponse.finishing,
        hasBundledDependencies: !!pkgSnapshot.bundledDependencies,
        independent,
        isBuilt: !!cache,
        modules,
        optional: !!pkgSnapshot.optional,
        optionalDependencies: new Set(R.keys(pkgSnapshot.optionalDependencies)),
        peripheralLocation,
        pkgId,
      }
    }
    const ctx = {
      force: opts.force,
      graph,
      independentLeaves: opts.independentLeaves,
      nodeModules,
      pkgSnapshotsByRelDepPaths: shr.packages,
      prefix: opts.prefix,
      registry: shr.registry,
      store: opts.store,
    }
    for (const depPath of R.keys(graph)) {
      const pkgSnapshot = pkgSnapshotByDepPath[depPath]
      const allDeps = {...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies}

      graph[depPath].children = await getChildrenPaths(ctx, allDeps)
    }
    const rootDeps = {...shr.devDependencies, ...shr.dependencies, ...shr.optionalDependencies}
    rootDependencies = await getChildrenPaths(ctx, rootDeps)
  }
  return {graph, rootDependencies}
}

async function getChildrenPaths (
  ctx: {
    graph: DepGraphNodesByDepPath,
    force: boolean,
    registry: string,
    nodeModules: string,
    independentLeaves: boolean,
    store: string,
    pkgSnapshotsByRelDepPaths: {[relDepPath: string]: PackageSnapshot},
    prefix: string,
  },
  allDeps: {[alias: string]: string},
) {
  const children: {[alias: string]: string} = {}
  for (const alias of R.keys(allDeps)) {
    const childDepPath = dp.refToAbsolute(allDeps[alias], alias, ctx.registry)
    const childRelDepPath = dp.relative(ctx.registry, childDepPath)
    const childPkgSnapshot = ctx.pkgSnapshotsByRelDepPaths[childRelDepPath]
    if (ctx.graph[childDepPath]) {
      children[alias] = ctx.graph[childDepPath].peripheralLocation
    } else if (ctx.independentLeaves && pkgIsIndependent(childPkgSnapshot)) {
      const pkgId = childPkgSnapshot.id || childDepPath
      const cache = !ctx.force && await getCache(ctx.store, pkgId)
      const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
      const inStoreLocation = pkgIdToFilename(pkgId)
      children[alias] = cache || path.join(inStoreLocation, 'node_modules', pkgName)
    } else if (childPkgSnapshot) {
      const relDepPath = dp.relative(ctx.registry, childDepPath)
      const pkgName = nameVerFromPkgSnapshot(relDepPath, childPkgSnapshot).name
      children[alias] = path.join(ctx.nodeModules, `.${pkgIdToFilename(childDepPath)}`, 'node_modules', pkgName)
    } else if (allDeps[alias].indexOf('link:') === 0 || allDeps[alias].indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.prefix, allDeps[alias].substr(5))
    } else {
      throw new Error(`${childRelDepPath} not found in shrinkwrap.yaml`)
    }
  }
  return children
}

async function getCache (storePath: string, pkgId: string) {
  return (await getCacheByEngine(storePath, pkgId))[ENGINE_NAME] as string
}

function pkgIsIndependent (pkgSnapshot: PackageSnapshot) {
  return pkgSnapshot.dependencies === undefined && pkgSnapshot.optionalDependencies === undefined
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

      if (depNode.independent) return
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
    R.values(depGraph)
      .map((depNode) => limitLinking(async () => {
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
            .map((target) => linkPackageBins(target, binPath)),
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
              // if (!pkg.installable) return
              await symlinkDependencyTo(alias, childrenToLink[alias], depNode.modules)
            }),
        )
      })),
  )
}

function symlinkDependencyTo (alias: string, peripheralLocation: string, dest: string) {
  dest = path.join(dest, alias)
  return symlinkDir(peripheralLocation, dest)
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
