import readPkgUp = require('read-pkg-up')
import path = require('path')
import thenify = require('thenify')
import lockfile = require('lockfile')
const lock = thenify(lockfile.lock)
const unlock = thenify(lockfile.unlock)
import semver = require('semver')
import requireJson from '../fs/require_json'
import writeJson from '../fs/write_json'
import expandTilde from '../fs/expand_tilde'
import resolveGlobalPkgPath from '../resolve_global_pkg_path'

import initLogger, {LoggerType} from '../logger'
import storeJsonController, {StoreJsonCtrl} from '../fs/store_json_controller'
import mkdirp from '../fs/mkdirp'
import {Dependencies} from '../install_multiple'
import {StoreDependencies} from '../fs/store_json_controller';

export type Package = {
  name: string,
  version: string,
  bin?: string | {
    [name: string]: string
  },
  dependencies?: Dependencies,
  devDependencies?: Dependencies,
  optionalDependencies?: Dependencies,
  scripts?: {
    [name: string]: string
  }
}

export type PackageAndPath = {
  pkg: Package,
  path: string
}

export type CommandContext = {
  store?: string,
  root?: string,
  pnpm?: string,
  dependents?: StoreDependencies,
  dependencies?: StoreDependencies
}

export type CommandNamespace = {
  pkg?: PackageAndPath,
  ctx: CommandContext,
  unlock?(): void,
  storeJsonCtrl?: StoreJsonCtrl
}

export type BasicOptions = {
  cwd: string,
  global: boolean,
  globalPath: string,
  storePath: string,
  quiet: boolean,
  logger: LoggerType,
  ignoreScripts: boolean
}

export default (opts: BasicOptions): Promise<CommandNamespace> => {
  const cwd = opts.cwd || process.cwd()
  const cmd: CommandNamespace = {
    ctx: {}
  }
  let lockfile: string
  return (opts.global ? readGlobalPkg(opts.globalPath) : readPkgUp({ cwd }))
    .then((_: PackageAndPath) => { cmd.pkg = _ })
    .then(() => updateContext())
    .then(() => mkdirp(cmd.ctx.store))
    .then(() => lock(lockfile))
    .then(() => cmd)

  function updateContext () {
    const root = cmd.pkg.path ? path.dirname(cmd.pkg.path) : cwd
    cmd.ctx.root = root
    cmd.ctx.store = resolveStorePath(opts.storePath)
    lockfile = path.resolve(cmd.ctx.store, 'lock')
    cmd.unlock = () => unlock(lockfile)
    cmd.storeJsonCtrl = storeJsonController(cmd.ctx.store)
    const storeJson = cmd.storeJsonCtrl.read()

    if (storeJson) {
      failIfNotCompatible(storeJson.pnpm)
    }

    Object.assign(cmd.ctx, storeJson)
    if (!opts.quiet) initLogger(opts.logger)

    function resolveStorePath (storePath: string) {
      if (storePath.indexOf('~/') === 0) {
        return expandTilde(storePath)
      }
      return path.resolve(root, storePath)
    }
  }
}

function failIfNotCompatible (storeVersion: string) {
  if (!storeVersion || !semver.satisfies(storeVersion, '>=0.28')) {
    throw new Error(`The store structure was changed.
      Remove it and run pnpm again.
      More info about what was changed at: https://github.com/rstacruz/pnpm/issues/276
      TIPS:
        If you have a shared store, remove both the node_modules and the shared shore.
        Otherwise just run \`rm -rf node_modules\``)
  }
}

function readGlobalPkg (globalPath: string) {
  if (!globalPath) throw new Error('globalPath is required')
  const globalPnpm = resolveGlobalPkgPath(globalPath)
  const globalPkgPath = path.resolve(globalPnpm, 'package.json')
  return readGlobalPkgJson(globalPkgPath)
    .then(globalPkgJson => ({
      pkg: globalPkgJson,
      path: globalPkgPath
    }))
}

function readGlobalPkgJson (globalPkgPath: string) {
  try {
    const globalPkgJson = requireJson(globalPkgPath)
    return Promise.resolve(globalPkgJson)
  } catch (err) {
    const pkgJson = {}
    return mkdirp(path.dirname(globalPkgPath))
      .then(_ => writeJson(globalPkgPath, pkgJson))
      .then(_ => pkgJson)
  }
}
