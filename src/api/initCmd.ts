import readPkgUp = require('read-pkg-up')
import path = require('path')
import semver = require('semver')
import {stripIndent} from 'common-tags'
import requireJson from '../fs/requireJson'
import writeJson from '../fs/writeJson'
import expandTilde from '../fs/expandTilde'
import resolveGlobalPkgPath from '../resolveGlobalPkgPath'
import {StrictPnpmOptions} from '../types'
import initLogger from '../logger'
import storeJsonController, {StoreJsonCtrl} from '../fs/storeJsonController'
import mkdirp from '../fs/mkdirp'
import {Package} from '../types'
import {StoreJson} from '../fs/storeJsonController'
import pnpmPkgJson from '../pnpmPkgJson'

export type PackageAndPath = {
  pkg: Package,
  path: string
}

export type CommandContext = {
  store: string,
  root: string,
  storeJson: StoreJson
}

export type CommandNamespace = {
  pkg?: PackageAndPath,
  ctx: CommandContext,
  storeJsonCtrl: StoreJsonCtrl
}

export default async function (opts: StrictPnpmOptions): Promise<CommandNamespace> {
  const cwd = opts.cwd || process.cwd()
  const pkg = await (opts.global ? readGlobalPkg(opts.globalPath) : readPkgUp({ cwd }))
  const root = pkg.path ? path.dirname(pkg.path) : cwd
  const store = resolveStorePath(opts.storePath, root)
  const storeJsonCtrl = storeJsonController(store)
  const storeJson = storeJsonCtrl.read()
  if (storeJson) {
    failIfNotCompatible(storeJson.pnpm)
  }
  const cmd: CommandNamespace = {
    pkg,
    ctx: {
      root,
      store,
      storeJson: storeJson || {
        pnpm: pnpmPkgJson.version,
        dependents: {},
        dependencies: {}
      }
    },
    storeJsonCtrl
  }

  if (!opts.quiet) initLogger(opts.logger)

  await mkdirp(cmd.ctx.store)
  return cmd
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
  const globalPnpm = resolveGlobalPkgPath(globalPath)
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

function resolveStorePath (storePath: string, pkgRoot: string) {
  if (storePath.indexOf('~/') === 0) {
    return expandTilde(storePath)
  }
  return path.resolve(pkgRoot, storePath)
}
