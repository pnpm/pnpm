import rimraf = require('rimraf-then')
import path = require('path')
import seq = require('promisequence')
import RegClient = require('npm-registry-client')
import logger from 'pnpm-logger'
import cloneDeep = require('lodash.clonedeep')
import globalBinPath = require('global-bin-path')
import {PnpmOptions, StrictPnpmOptions, Dependencies, LifecycleHooks, InstalledPackage} from '../types'
import createGot from '../network/got'
import getContext, {PnpmContext} from './getContext'
import installMultiple from '../install/installMultiple'
import save from '../save'
import linkPeers from '../install/linkPeers'
import runtimeError from '../runtimeError'
import getSaveType from '../getSaveType'
import {sync as runScriptSync} from '../runScript'
import postInstall from '../install/postInstall'
import extendOptions from './extendOptions'
import pnpmPkgJson from '../pnpmPkgJson'
import lock from './lock'
import {save as saveGraph, Graph} from '../fs/graphController'
import {read as readStore, save as saveStore} from '../fs/storeController'
import {save as saveShrinkwrap, Shrinkwrap} from '../fs/shrinkwrap'
import {save as saveModules} from '../fs/modulesController'
import {tryUninstall, removePkgFromStore} from './uninstall'
import mkdirp from '../fs/mkdirp'
import {CachedPromises} from '../memoize'
import linkBins from '../install/linkBins'

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type InstallContext = {
  installs: InstalledPackages,
  installationSequence: string[],
  fetchLocks: CachedPromises<void>,
  graph: Graph,
  lifecycle: LifecycleHooks,
  shrinkwrap: Shrinkwrap,
  resolutionLinked: CachedPromises<void>,
  installed: Set<string>,
}

export async function install (maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  const installCtx = await createInstallCmd(opts, ctx.graph, ctx.shrinkwrap, ctx.cache)

  if (!ctx.pkg) throw runtimeError('No package.json found')
  const packagesToInstall = Object.assign({}, ctx.pkg.dependencies || {})
  if (!opts.production) Object.assign(packagesToInstall, ctx.pkg.devDependencies || {})

  return lock(ctx.storePath, () => installInContext('general', packagesToInstall, ctx, installCtx, opts))
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
  const installCtx = await createInstallCmd(opts, ctx.graph, ctx.shrinkwrap, ctx.cache)

  return lock(ctx.storePath, () => installInContext('named', packagesToInstall, ctx, installCtx, opts))
}

async function installInContext (installType: string, packagesToInstall: Dependencies, ctx: PnpmContext, installCtx: InstallContext, opts: StrictPnpmOptions) {
  // TODO: ctx.graph should not be muted. installMultiple should return a new graph
  const oldGraph: Graph = cloneDeep(ctx.graph)
  const nodeModulesPath = path.join(ctx.root, 'node_modules')
  const client = new RegClient(adaptConfig(opts))
  const pkgs: InstalledPackage[] = await lock(ctx.cache, async function () {
    const installOpts = {
      linkLocal: opts.linkLocal,
      dependent: ctx.root,
      root: ctx.root,
      storePath: ctx.storePath,
      force: opts.force,
      depth: opts.depth,
      tag: opts.tag,
      engineStrict: opts.engineStrict,
      nodeVersion: opts.nodeVersion,
      lifecycle: opts.lifecycle,
      got: createGot(client, {
        cachePath: ctx.cache,
        cacheTTL: opts.cacheTTL
      }),
      fetchingFiles: Promise.resolve(),
      nodeModulesStore: path.join(nodeModulesPath, '.resolutions'),
    }
    const installedPkgs = await installMultiple(
      installCtx,
      packagesToInstall,
      ctx.pkg && ctx.pkg && ctx.pkg.optionalDependencies || {},
      nodeModulesPath,
      installOpts
    )
    const binPath = opts.global ? globalBinPath() : path.join(nodeModulesPath, '.bin')
    await linkBins(nodeModulesPath, binPath)
    return installedPkgs
  })

  if (installType === 'named') {
    const saveType = getSaveType(opts)
    if (saveType) {
      if (!ctx.pkg) {
        throw new Error('Cannot save because no package.json found')
      }
      const inputNames = Object.keys(packagesToInstall)
      const savedPackages = pkgs.filter((pkg: InstalledPackage) => inputNames.indexOf(pkg.pkg.name) > -1)
      const pkgJsonPath = path.join(ctx.root, 'package.json')
      await save(pkgJsonPath, savedPackages, saveType, opts.saveExact)
    }
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
    await seq(
      installCtx.installationSequence.map(async pkgId => {
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
      })
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

  if (opts.lifecycle.installDidComplete) {
    await opts.lifecycle.installDidComplete(installCtx.installs)
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

async function createInstallCmd (opts: StrictPnpmOptions, graph: Graph, shrinkwrap: Shrinkwrap, cache: string): Promise<InstallContext> {
  return {
    fetchLocks: {},
    installLocks: {},
    installs: {},
    lifecycle: opts.lifecycle,
    graph,
    shrinkwrap,
    resolutionLinked: {},
    installed: new Set(),
    installationSequence: [],
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
