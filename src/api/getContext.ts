import readPkgUp = require('read-pkg-up')
import path = require('path')
import semver = require('semver')
import {stripIndent} from 'common-tags'
import requireJson from '../fs/requireJson'
import writePkg = require('write-pkg')
import expandTilde, {isHomepath} from '../fs/expandTilde'
import {StrictPnpmOptions} from '../types'
import {
  read as readGraph,
  Graph,
} from '../fs/graphController'
import {
  read as readShrinkwrap,
  Shrinkwrap,
} from '../fs/shrinkwrap'
import {
  read as readModules,
} from '../fs/modulesController'
import mkdirp from '../fs/mkdirp'
import {Package} from '../types'
import normalizePath = require('normalize-path')

export type PnpmContext = {
  pkg?: Package,
  cache: string,
  storePath: string,
  root: string,
  graph: Graph,
  shrinkwrap: Shrinkwrap,
  isFirstInstallation: boolean,
}

export default async function (opts: StrictPnpmOptions): Promise<PnpmContext> {
  const pkg = await (opts.global ? readGlobalPkg(opts.globalPath) : readPkgUp({ cwd: opts.cwd }))
  const root = normalizePath(pkg.path ? path.dirname(pkg.path) : opts.cwd)
  const storeBasePath = resolveStoreBasePath(opts.storePath, root)

  const storePath = getStorePath(storeBasePath)

  let modules = await readModules(path.join(root, 'node_modules'))
  const isFirstInstallation: boolean = !modules
  if (modules && modules.storePath !== storePath) {
    const err = new Error(`The package's modules are from store at ${modules.storePath} and you are trying to use store at ${storePath}`)
    err['code'] = 'ALIEN_STORE'
    throw err
  }
  if (modules) {
    if (!modules.packageManager) {
      const msg = structureChangeMsg(stripIndent`
        The change was needed to allow machine stores and dependency locks:
          PR: https://github.com/pnpm/pnpm/pull/524
      `)
      throw new Error(msg)
    }
    const pnpmVersion = getPackageManagerVersion(modules.packageManager)
    failIfNotCompatibleStore(pnpmVersion)
    failIfNotCompatibleNodeModules(pnpmVersion)
  }

  function getPackageManagerVersion(packageManager: string) {
    // handle the case when the package is scoped: @scope/pkgname
    if (packageManager.startsWith('@')) {
      return packageManager.split('@')[2]
    } else {
      return packageManager.split('@')[1]
    }
  }

  const graph = await readGraph(path.join(root, 'node_modules')) || {}
  const shrinkwrap = await readShrinkwrap(root) || {}
  const ctx: PnpmContext = {
    pkg: pkg.pkg,
    root,
    cache: expandTilde(opts.cachePath),
    storePath,
    graph,
    shrinkwrap,
    isFirstInstallation,
  }

  await mkdirp(ctx.cache)
  await mkdirp(ctx.storePath)
  return ctx
}

function failIfNotCompatibleStore (pnpmVersion: string) {
  if (!pnpmVersion || !semver.satisfies(pnpmVersion, '>=0.28')) {
    const msg = structureChangeMsg('More info about what was changed at: https://github.com/pnpm/pnpm/issues/276')
    throw new Error(msg)
  }
  if (!semver.satisfies(pnpmVersion, '>=0.33')) {
    const msg = structureChangeMsg(stripIndent`
      The change was needed to fix the GitHub rate limit issue:
        Issue: https://github.com/pnpm/pnpm/issues/361
        PR: https://github.com/pnpm/pnpm/pull/363
    `)
    throw new Error(msg)
  }
  if (!semver.satisfies(pnpmVersion, '>=0.37')) {
    const msg = structureChangeMsg(stripIndent`
      The structure of store.json/dependencies was changed to map dependencies to their fullnames
    `)
    throw new Error(msg)
  }
  if (!semver.satisfies(pnpmVersion, '>=0.38')) {
    const msg = structureChangeMsg(stripIndent`
      The structure of store.json/dependencies was changed to not include the redundunt package.json at the end
    `)
    throw new Error(msg)
  }
}

function failIfNotCompatibleNodeModules (pnpmVersion: string) {
  if (!pnpmVersion || !semver.satisfies(pnpmVersion, '>=0.48')) {
    throw new Error(stripIndent`
      The node_modules structure was changed.
      Remove it and run pnpm again.
      Related PR: https://github.com/pnpm/pnpm/pull/534
      TIPS: you can run \`rm -rf node_modules\`
    `)
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

const DefaultGlobalPkg: Package = {
  name: 'pnpm-global-pkg',
  version: '1.0.0',
  private: true,
}

async function readGlobalPkgJson (globalPkgPath: string) {
  try {
    const globalPkgJson = await requireJson(globalPkgPath)
    return globalPkgJson
  } catch (err) {
    await mkdirp(path.dirname(globalPkgPath))
    await writePkg(globalPkgPath, DefaultGlobalPkg)
    return DefaultGlobalPkg
  }
}

function resolveStoreBasePath (storePath: string, pkgRoot: string) {
  if (isHomepath(storePath)) {
    return expandTilde(storePath)
  }
  return path.resolve(pkgRoot, storePath)
}

function getStorePath (storeBasePath: string): string {
  if (underNodeModules(storeBasePath)) {
    return storeBasePath
  }
  return path.join(storeBasePath, '1')
}

function underNodeModules (dirpath: string): boolean {
  return dirpath.split(path.sep).indexOf('node_modules') !== -1
}
