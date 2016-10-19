import readPkgUp = require('read-pkg-up')
import path = require('path')
import semver = require('semver')
import {stripIndent} from 'common-tags'
import requireJson from '../fs/requireJson'
import writeJson from '../fs/writeJson'
import expandTilde, {isHomepath} from '../fs/expandTilde'
import {StrictPnpmOptions} from '../types'
import initLogger from '../logger'
import {
  read as readStore,
  create as createStore,
  Store,
  TreeType,
} from '../fs/storeController'
import {
  read as readModules
} from '../fs/modulesController'
import mkdirp from '../fs/mkdirp'
import {Package} from '../types'
import {getCachePath} from './cache'
import normalizePath = require('normalize-path')

export type PnpmContext = {
  pkg?: Package,
  cache: string,
  storePath: string,
  root: string,
  store: Store,
  isFirstInstallation: boolean,
}

export default async function (opts: StrictPnpmOptions): Promise<PnpmContext> {
  const pkg = await (opts.global ? readGlobalPkg(opts.globalPath) : readPkgUp({ cwd: opts.cwd }))
  const root = normalizePath(pkg.path ? path.dirname(pkg.path) : opts.cwd)
  const storeBasePath = resolveStoreBasePath(opts.storePath, root)

  // to avoid orphan packages created with pnpm v0.41.0 and earlier
  if (!underNodeModules(storeBasePath) && await readStore(storeBasePath)) {
    throw new Error(structureChangeMsg('Shared stores were divided into types, flat and nested. https://github.com/rstacruz/pnpm/pull/429'))
  }

  const treeType: TreeType = opts.flatTree ? 'flat' : 'nested'
  const storePath = getStorePath(treeType, storeBasePath)

  let modules = await readModules(path.join(root, 'node_modules'))
  const isFirstInstallation: boolean = !modules
  if (modules && modules.storePath !== storePath) {
    const err = new Error(`The package's modules are from store at ${modules.storePath} and you are trying to use store at ${storePath}`)
    err['code'] = 'ALIEN_STORE'
    throw err
  }

  const store = await readStore(storePath) || createStore(treeType)
  store.type = store.type || 'nested' // for backward compatibility with v0.41.0 and earlier
  if (store.type !== treeType) {
    const err = new Error(`Cannot use a ${store.type} store for a ${treeType} installation`)
    err['code'] = 'INCONSISTENT_TREE_TYPE'
    throw err
  }
  if (store) {
    failIfNotCompatible(store.pnpm)
  }
  const ctx: PnpmContext = {
    pkg: pkg.pkg,
    root,
    cache: getCachePath(opts.globalPath),
    storePath,
    store,
    isFirstInstallation,
  }

  if (!opts.silent) initLogger(opts.logger)

  await mkdirp(ctx.cache)
  await mkdirp(ctx.storePath)
  return ctx
}

function failIfNotCompatible (storeVersion: string) {
  if (!storeVersion || !semver.satisfies(storeVersion, '>=0.28')) {
    const msg = structureChangeMsg('More info about what was changed at: https://github.com/rstacruz/pnpm/issues/276')
    throw new Error(msg)
  }
  if (!semver.satisfies(storeVersion, '>=0.33')) {
    const msg = structureChangeMsg(stripIndent`
      The change was needed to fix the GitHub rate limit issue:
        Issue: https://github.com/rstacruz/pnpm/issues/361
        PR: https://github.com/rstacruz/pnpm/pull/363
    `)
    throw new Error(msg)
  }
  if (!semver.satisfies(storeVersion, '>=0.37')) {
    const msg = structureChangeMsg(stripIndent`
      The structure of store.json/dependencies was changed to map dependencies to their fullnames
    `)
    throw new Error(msg)
  }
  if (!semver.satisfies(storeVersion, '>=0.38')) {
    const msg = structureChangeMsg(stripIndent`
      The structure of store.json/dependencies was changed to not include the redundunt package.json at the end
    `)
    throw new Error(msg)
  }
}

function structureChangeMsg (moreInfo: string): string {
  return stripIndent`
    The store structure was changed.
    Remove it and run pnpm again.
    ${moreInfo}
    TIPS:
      If you have a shared store, remove both the node_modules and the shared store.
      Otherwise just run \`rm -rf node_modules\`
  `
}

async function readGlobalPkg (globalPath: string) {
  if (!globalPath) throw new Error('globalPath is required')
  const globalPnpm = expandTilde(globalPath)
  const globalPkgPath = path.resolve(globalPnpm, 'package.json')
  const globalPkgJson = await readGlobalPkgJson(globalPkgPath)
  return {
    pkg: globalPkgJson,
    path: globalPkgPath
  }
}

async function readGlobalPkgJson (globalPkgPath: string) {
  try {
    const globalPkgJson = requireJson(globalPkgPath)
    return globalPkgJson
  } catch (err) {
    const pkgJson = {}
    await mkdirp(path.dirname(globalPkgPath))
    await writeJson(globalPkgPath, pkgJson)
    return pkgJson
  }
}

function resolveStoreBasePath (storePath: string, pkgRoot: string) {
  if (isHomepath(storePath)) {
    return expandTilde(storePath)
  }
  return path.resolve(pkgRoot, storePath)
}

function getStorePath (treeType: TreeType, storeBasePath: string): string {
  // potentially shared stores have to have separate subdirs for different
  // dependency tree types 
  if (!underNodeModules(storeBasePath)) {
    return path.join(storeBasePath, treeType)
  }
  return storeBasePath
}

function underNodeModules (dirpath: string): boolean {
  return dirpath.split(path.sep).indexOf('node_modules') !== -1
}
