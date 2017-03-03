import rimraf = require('rimraf-then')
import path = require('path')
import RegClient = require('npm-registry-client')
import logger from 'pnpm-logger'
import cloneDeep = require('lodash.clonedeep')
import globalBinPath = require('global-bin-path')
import pLimit = require('p-limit')
import {PnpmOptions, StrictPnpmOptions, Dependencies} from '../types'
import createGot from '../network/got'
import getContext, {PnpmContext} from './getContext'
import installMultiple, {InstalledPackage} from '../install/installMultiple'
import save from '../save'
import linkPeers from '../install/linkPeers'
import getSaveType from '../getSaveType'
import {sync as runScriptSync} from '../runScript'
import postInstall from '../install/postInstall'
import extendOptions from './extendOptions'
import pnpmPkgJson from '../pnpmPkgJson'
import lock from './lock'
import {save as saveGraph, Graph} from '../fs/graphController'
import {read as readStore, save as saveStore} from '../fs/storeController'
import {
  save as saveShrinkwrap,
  Shrinkwrap,
  ResolvedDependencies,
} from '../fs/shrinkwrap'
import {save as saveModules} from '../fs/modulesController'
import {tryUninstall, removePkgFromStore} from './uninstall'
import mkdirp = require('mkdirp-promise')
import createMemoize, {MemoizedFunc} from '../memoize'
import linkBins from '../install/linkBins'
import {Package} from '../types'

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type InstallContext = {
  installs: InstalledPackages,
  installationSequence: string[],
  graph: Graph,
  shrinkwrap: Shrinkwrap,
  installed: Set<string>,
  fetchingLocker: MemoizedFunc<Boolean>,
  linkingLocker: MemoizedFunc<void>,
}

export async function install (maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  const installCtx = await createInstallCmd(opts, ctx.graph, ctx.shrinkwrap)

  if (!ctx.pkg) throw new Error('No package.json found')
  const packagesToInstall = Object.assign({}, ctx.pkg.dependencies || {})
  if (!opts.production) Object.assign(packagesToInstall, ctx.pkg.devDependencies || {})

  return lock(
    ctx.storePath,
    () => installInContext('general', packagesToInstall, ctx, installCtx, opts),
    {stale: opts.lockStaleDuration}
  )
}

/**
 * Perform installation.
 *
 * @example
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { silent: true })
 */
export async function installPkgs (fuzzyDeps: string[] | Dependencies, maybeOpts?: PnpmOptions) {
  let packagesToInstall = mapify(fuzzyDeps)
  if (!Object.keys(packagesToInstall).length) {
    throw new Error('At least one package has to be installed')
  }
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  const installCtx = await createInstallCmd(opts, ctx.graph, ctx.shrinkwrap)

  return lock(
    ctx.storePath,
    () => installInContext('named', packagesToInstall, ctx, installCtx, opts),
    {stale: opts.lockStaleDuration}
  )
}

function getResolutions(
  packagesToInstall: Dependencies,
  resolvedSpecDeps: ResolvedDependencies
): ResolvedDependencies {
  return Object.keys(packagesToInstall)
    .reduce((resolvedDeps, depName) => {
      const spec = `${depName}@${packagesToInstall[depName]}`
      if (resolvedSpecDeps[spec]) {
        resolvedDeps[depName] = resolvedSpecDeps[spec]
      }
      return resolvedDeps
    }, {})
}

async function installInContext (installType: string, packagesToInstall: Dependencies, ctx: PnpmContext, installCtx: InstallContext, opts: StrictPnpmOptions) {
  // TODO: ctx.graph should not be muted. installMultiple should return a new graph
  const oldGraph: Graph = cloneDeep(ctx.graph)
  const nodeModulesPath = path.join(ctx.root, 'node_modules')
  const client = new RegClient(adaptConfig(opts))

  const optionalDependencies = ctx.pkg && ctx.pkg && ctx.pkg.optionalDependencies || {}
  const resolvedDependencies: ResolvedDependencies | undefined = installType !== 'general'
    ? undefined
    : getResolutions(
      Object.assign({}, optionalDependencies, packagesToInstall),
      ctx.shrinkwrap.dependencies
    )
  const installOpts = {
    root: ctx.root,
    storePath: ctx.storePath,
    localRegistry: opts.localRegistry,
    force: opts.force,
    depth: opts.depth,
    tag: opts.tag,
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    got: createGot(client, {networkConcurrency: opts.networkConcurrency}),
    baseNodeModules: nodeModulesPath,
    metaCache: opts.metaCache,
    resolvedDependencies,
  }
  const pkgs: InstalledPackage[] = await installMultiple(
    installCtx,
    packagesToInstall,
    optionalDependencies,
    nodeModulesPath,
    installOpts
  )
  const binPath = opts.global ? globalBinPath() : path.join(nodeModulesPath, '.bin')
  await linkBins(nodeModulesPath, binPath)

  let newPkg: Package | undefined = ctx.pkg
  if (installType === 'named') {
    const saveType = getSaveType(opts)
    if (saveType) {
      if (!ctx.pkg) {
        throw new Error('Cannot save because no package.json found')
      }
      const inputNames = Object.keys(packagesToInstall)
      const savedPackages = pkgs.filter((pkg: InstalledPackage) => inputNames.indexOf(pkg.pkg.name) > -1)
      const pkgJsonPath = path.join(ctx.root, 'package.json')
      newPkg = await save(pkgJsonPath, savedPackages, saveType, opts.saveExact)
    }
  }

  if (pkgs && pkgs.length && newPkg) {
    ctx.shrinkwrap.dependencies = ctx.shrinkwrap.dependencies || {}

    const deps = newPkg.dependencies || {}
    const devDeps = newPkg.devDependencies || {}
    const optionalDeps = newPkg.optionalDependencies || {}

    const getSpecFromPkg = (depName: string) => deps[depName] || devDeps[depName] || optionalDeps[depName]

    pkgs.forEach(dep => {
      const spec = getSpecFromPkg(dep.pkg.name)
      if (spec) {
        ctx.shrinkwrap.dependencies[`${dep.pkg.name}@${spec}`] = dep.id
      }
    })
    Object.keys(ctx.shrinkwrap.dependencies)
      .forEach(depSpec => {
        const parts = depSpec.split('@')
        const depName = parts[0]
        const currentDepSpec = parts[1]
        if (getSpecFromPkg(depName) !== currentDepSpec) {
          delete ctx.shrinkwrap.dependencies[depSpec]
        }
      })
  }

  const newGraph = Object.assign({}, ctx.graph)
  await removeOrphanPkgs(oldGraph, newGraph, ctx.root, ctx.storePath)
  await saveGraph(path.join(ctx.root, 'node_modules'), newGraph)
  await saveShrinkwrap(ctx.root, ctx.shrinkwrap)
  if (ctx.isFirstInstallation) {
    await saveModules(path.join(ctx.root, 'node_modules'), {
      packageManager: `${pnpmPkgJson.name}@${pnpmPkgJson.version}`,
      storePath: ctx.storePath,
    })
  }

  await linkPeers(installCtx.installs)

  // postinstall hooks
  if (!(opts.ignoreScripts || !installCtx.installationSequence || !installCtx.installationSequence.length)) {
    const limitChild = pLimit(opts.childConcurrency)
    await Promise.all(
      installCtx.installationSequence.map(pkgId => limitChild(async () => {
        try {
          await postInstall(installCtx.installs[pkgId].hardlinkedLocation, installLogger(pkgId))
        } catch (err) {
          if (installCtx.installs[pkgId].optional) {
            logger.warn({
              message: `Skipping failed optional dependency ${pkgId}`,
              err,
            })
            return
          }
          throw err
        }
      }))
    )
  }
  if (!opts.ignoreScripts && ctx.pkg) {
    const scripts = ctx.pkg && ctx.pkg.scripts || {}

    if (scripts['postinstall']) {
      npmRun('postinstall', ctx.root)
    }
    if (installType === 'general' && scripts['prepublish']) {
      npmRun('prepublish', ctx.root)
    }
  }
}

async function removeOrphanPkgs (oldGraphJson: Graph, newGraphJson: Graph, root: string, storePath: string) {
  const oldPkgIds = new Set(Object.keys(oldGraphJson))
  const newPkgIds = new Set(Object.keys(newGraphJson))

  const store = await readStore(storePath) || {}
  const notDependents = difference(oldPkgIds, newPkgIds)

  await Promise.all(Array.from(notDependents).map(async function (notDependent) {
    if (store[notDependent]) {
      store[notDependent].splice(store[notDependent].indexOf(root), 1)
      if (!store[notDependent].length) {
        delete store[notDependent]
        await rimraf(path.join(storePath, notDependent))
      }
    }
  }))

  const newDependents = difference(newPkgIds, oldPkgIds)

  newDependents.forEach(newDependent => {
    store[newDependent] = store[newDependent] || []
    if (store[newDependent].indexOf(root) === -1) {
      store[newDependent].push(root)
    }
  })

  await saveStore(storePath, store)
}

function difference<T> (setA: Set<T>, setB: Set<T>) {
  const difference = new Set(setA)
  for (const elem of setB) {
    difference.delete(elem)
  }
  return difference
}

async function createInstallCmd (opts: StrictPnpmOptions, graph: Graph, shrinkwrap: Shrinkwrap): Promise<InstallContext> {
  return {
    installs: {},
    graph,
    shrinkwrap,
    installed: new Set(),
    installationSequence: [],
    fetchingLocker: createMemoize<boolean>(opts.fetchingConcurrency),
    linkingLocker: createMemoize<void>(),
  }
}

function adaptConfig (opts: StrictPnpmOptions) {
  const registryLog = logger('registry')
  return {
    proxy: {
      http: opts.proxy,
      https: opts.httpsProxy,
      localAddress: opts.localAddress
    },
    ssl: {
      certificate: opts.cert,
      key: opts.key,
      ca: opts.ca,
      strict: opts.strictSsl
    },
    retry: {
      count: opts.fetchRetries,
      factor: opts.fetchRetryFactor,
      minTimeout: opts.fetchRetryMintimeout,
      maxTimeout: opts.fetchRetryMaxtimeout
    },
    userAgent: opts.userAgent,
    log: Object.assign({}, registryLog, {
      verbose: registryLog.debug.bind(null, 'http'),
      http: registryLog.debug.bind(null, 'http'),
    }),
    defaultTag: opts.tag
  }
}

function npmRun (scriptName: string, pkgRoot: string) {
  const result = runScriptSync('npm', ['run', scriptName], {
    cwd: pkgRoot,
    stdio: 'inherit'
  })
  if (result.status !== 0) {
    process.exit(result.status)
  }
}

const lifecycleLogger = logger('lifecycle')

function installLogger (pkgId: string) {
  return (stream: string, line: string) => {
    const logLevel = stream === 'stderr' ? 'error' : 'info'
    lifecycleLogger[logLevel]({pkgId, line})
  }
}

function mapify (pkgs: string[] | Dependencies): Dependencies {
  if (!pkgs) return {}
  if (Array.isArray(pkgs)) {
    return pkgs.reduce((pkgsMap: Dependencies, pkgRequest: string) => {
      const matches = /(@?[^@]+)@(.*)/.exec(pkgRequest)
      if (!matches) {
        pkgsMap[pkgRequest] = '*'
      } else {
        pkgsMap[matches[1]] = matches[2]
      }
      return pkgsMap
    }, {})
  }
  return pkgs
}
