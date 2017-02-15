import readPkgUp = require('read-pkg-up')
import path = require('path')
import readPkg from '../fs/readPkg'
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
import mkdirp = require('mkdirp-promise')
import {Package} from '../types'
import normalizePath = require('normalize-path')
import rimraf = require('rimraf-then')
import logger from 'pnpm-logger'
import checkCompatibility from './checkCompatibility'

export type PnpmContext = {
  pkg?: Package,
  storePath: string,
  root: string,
  graph: Graph,
  shrinkwrap: Shrinkwrap,
  isFirstInstallation: boolean,
}

export default async function getContext (opts: StrictPnpmOptions): Promise<PnpmContext> {
  const pkg = await (opts.global ? readGlobalPkg(opts.globalPath) : readPkgUp({ cwd: opts.cwd }))
  const root = normalizePath(pkg.path ? path.dirname(pkg.path) : opts.cwd)
  const storeBasePath = resolveStoreBasePath(opts.storePath, root)

  const storePath = getStorePath(storeBasePath)

  const modulesPath = path.join(root, 'node_modules')
  let modules = await readModules(modulesPath)
  const isFirstInstallation: boolean = !modules

  if (modules) {
    try {
      checkCompatibility(modules, {storePath, modulesPath})
    } catch (err) {
      if (opts.force) {
        logger.info(`Recreating ${modulesPath}`)
        await rimraf(modulesPath)
        return getContext(opts)
      }
      throw err
    }
  }

  const graph = await readGraph(path.join(root, 'node_modules')) || {}
  const shrinkwrap = await readShrinkwrap(root, {force: opts.force})
  const ctx: PnpmContext = {
    pkg: pkg.pkg,
    root,
    storePath,
    graph,
    shrinkwrap,
    isFirstInstallation,
  }

  await mkdirp(ctx.storePath)
  return ctx
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
    const globalPkgJson = await readPkg(globalPkgPath)
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
