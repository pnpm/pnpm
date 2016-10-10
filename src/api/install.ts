import path = require('path')
import seq = require('promisequence')
import chalk = require('chalk')
import createDebug = require('debug')
import RegClient = require('npm-registry-client')
import logger = require('@zkochan/logger')
import cloneDeep = require('lodash.clonedeep')
import {PnpmOptions, StrictPnpmOptions, Dependencies} from '../types'
import createGot from '../network/got'
import getContext, {PnpmContext} from './getContext'
import installMultiple from '../installMultiple'
import save from '../save'
import linkPeers from '../install/linkPeers'
import runtimeError from '../runtimeError'
import getSaveType from '../getSaveType'
import {sync as runScriptSync} from '../runScript'
import postInstall from '../install/postInstall'
import linkBins from '../install/linkBins'
import extendOptions from './extendOptions'
import {InstalledPackage} from '../install'
import {Got} from '../network/got'
import pnpmPkgJson from '../pnpmPkgJson'
import lock from './lock'
import {StoreJson} from '../fs/storeJsonController'
import {save as saveStoreJson} from '../fs/storeJsonController'
import {tryUninstall, removePkgFromStore} from './uninstall'

export type PackageInstallationResult = {
  path: string,
  pkgId: string
}

export type CachedPromises = {
  [name: string]: Promise<void>
}

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type InstallContext = {
  installs: InstalledPackages,
  piq?: PackageInstallationResult[],
  got: Got,
  builds: CachedPromises,
  fetches: CachedPromises,
  storeJson: StoreJson
}

export async function install (maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  const installCtx = await createInstallCmd(opts, ctx.storeJson)

  if (!ctx.pkg) throw runtimeError('No package.json found')
  const packagesToInstall = Object.assign({}, ctx.pkg.dependencies || {})
  if (!opts.production) Object.assign(packagesToInstall, ctx.pkg.devDependencies || {})

  return lock(ctx.store, () => installInContext('general', packagesToInstall, ctx, installCtx, opts))
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
  const installCtx = await createInstallCmd(opts, ctx.storeJson)

  return lock(ctx.store, () => installInContext('named', packagesToInstall, ctx, installCtx, opts))
}

async function installInContext (installType: string, packagesToInstall: Dependencies, ctx: PnpmContext, installCtx: InstallContext, opts: StrictPnpmOptions) {
  // TODO: ctx.storeJson should not be muted. installMultiple should return a new storeJson
  const oldStoreJson: StoreJson = cloneDeep(ctx.storeJson)
  const pkgs: InstalledPackage[] = await installMultiple(installCtx,
    packagesToInstall,
    ctx.pkg && ctx.pkg && ctx.pkg.optionalDependencies || {},
    path.join(ctx.root, 'node_modules'),
    {
      linkLocal: opts.linkLocal,
      dependent: ctx.root,
      root: ctx.root,
      store: ctx.store,
      force: opts.force,
      depth: opts.depth,
      tag: opts.tag
    }
  )

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

  const newStoreJson = Object.assign({}, ctx.storeJson, {
    pnpm: pnpmPkgJson.version
  })
  await removeOrphanPkgs(oldStoreJson, newStoreJson, ctx.root, ctx.store)
  saveStoreJson(ctx.store, newStoreJson)

  await linkPeers(ctx.store, installCtx.installs)
  // postinstall hooks
  if (!(opts.ignoreScripts || !installCtx.piq || !installCtx.piq.length)) {
    await seq(
      installCtx.piq.map(pkg => () => linkBins(path.join(pkg.path, '_', 'node_modules'))
          .then(() => postInstall(pkg.path, installLogger(pkg.pkgId)))
          .catch(err => {
            if (installCtx.installs[pkg.pkgId].optional) {
              console.log('Skipping failed optional dependency ' + pkg.pkgId + ':')
              console.log(err.message || err)
              return
            }
            throw err
          })
      ))
  }
  await linkBins(path.join(ctx.root, 'node_modules'))
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

function removeOrphanPkgs (oldStoreJson: StoreJson, newStoreJson: StoreJson, root: string, store: string) {
  const oldDeps = oldStoreJson.packages[root] && oldStoreJson.packages[root].dependencies || {}
  const newDeps = newStoreJson.packages[root] && newStoreJson.packages[root].dependencies || {}

  const maybeUninstallPkgs = Object.keys(oldDeps)
    .filter(depName => oldDeps[depName] !== newDeps[depName])
    .map(depName => oldDeps[depName])

  const uninstallPkgs = tryUninstall(maybeUninstallPkgs, newStoreJson, root)

  return Promise.all(uninstallPkgs.map(pkgId => removePkgFromStore(pkgId, store)))
}

async function createInstallCmd (opts: StrictPnpmOptions, storeJson: StoreJson): Promise<InstallContext> {
  const client = new RegClient(adaptConfig(opts))
  return {
    fetches: {},
    builds: {},
    installs: {},
    got: createGot(client),
    storeJson
  }
}

function adaptConfig (opts: StrictPnpmOptions) {
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
    log: Object.assign({}, logger, {
      verbose: logger.log.bind(null, 'verbose'),
      http: logger.log.bind(null, 'http')
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

function installLogger (pkgId: string) {
  return (stream: string, line: string) => {
    createDebug('pnpm:post_install')(`${pkgId} ${line}`)

    if (stream === 'stderr') {
      console.log(chalk.blue(pkgId) + '! ' + chalk.gray(line))
      return
    }
    console.log(chalk.blue(pkgId) + '  ' + chalk.gray(line))
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
