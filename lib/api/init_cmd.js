'use strict'
const readPkgUp = require('read-pkg-up')
const resolve = require('path').resolve
const dirname = require('path').dirname
const thenify = require('thenify')
const lock = thenify(require('lockfile').lock)
const unlock = thenify(require('lockfile').unlock)
const semver = require('semver')
const requireJson = require('../fs/require_json')
const writeJson = require('../fs/write_json')
const expandTilde = require('../fs/expand_tilde')
const resolveGlobalPkgPath = require('../resolve_global_pkg_path')

const initLogger = require('../logger')
const storeJsonController = require('../fs/store_json_controller')
const mkdirp = require('../fs/mkdirp')

module.exports = opts => {
  opts = opts || {}
  const cwd = opts.cwd || process.cwd()
  const cmd = {
    ctx: {}
  }
  let lockfile
  return (opts.global ? readGlobalPkg(opts.globalPath) : readPkgUp({ cwd }))
    .then(_ => { cmd.pkg = _ })
    .then(_ => updateContext())
    .then(_ => mkdirp(cmd.ctx.store))
    .then(_ => lock(lockfile))
    .then(_ => cmd)

  function updateContext () {
    const root = cmd.pkg.path ? dirname(cmd.pkg.path) : cwd
    cmd.ctx.root = root
    cmd.ctx.store = resolveStorePath(opts.storePath)
    lockfile = resolve(cmd.ctx.store, 'lock')
    cmd.unlock = () => unlock(lockfile)
    cmd.storeJsonCtrl = storeJsonController(cmd.ctx.store)
    const storeJson = cmd.storeJsonCtrl.read()

    if (storeJson) {
      failIfNotCompatible(storeJson.pnpm)
    }

    Object.assign(cmd.ctx, storeJson)
    if (!opts.quiet) initLogger(opts.logger)

    function resolveStorePath (storePath) {
      if (storePath.indexOf('~/') === 0) {
        return expandTilde(storePath)
      }
      return resolve(root, storePath)
    }
  }
}

function failIfNotCompatible (storeVersion) {
  if (!storeVersion || !semver.satisfies(storeVersion, '>=0.28')) {
    throw new Error(`The store structure was changed.
      Remove it and run pnpm again.
      More info about what was changed at: https://github.com/rstacruz/pnpm/issues/276
      TIPS:
        If you have a shared store, remove both the node_modules and the shared shore.
        Otherwise just run \`rm -rf node_modules\``)
  }
}

function readGlobalPkg (globalPath) {
  if (!globalPath) throw new Error('globalPath is required')
  const globalPnpm = resolveGlobalPkgPath(globalPath)
  const globalPkgPath = resolve(globalPnpm, 'package.json')
  return readGlobalPkgJson(globalPkgPath)
    .then(globalPkgJson => ({
      pkg: globalPkgJson,
      path: globalPkgPath
    }))
}

function readGlobalPkgJson (globalPkgPath) {
  try {
    const globalPkgJson = requireJson(globalPkgPath)
    return Promise.resolve(globalPkgJson)
  } catch (err) {
    const pkgJson = {}
    return mkdirp(dirname(globalPkgPath))
      .then(_ => writeJson(globalPkgPath, pkgJson))
      .then(_ => pkgJson)
  }
}
