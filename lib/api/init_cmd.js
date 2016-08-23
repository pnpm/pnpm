'use strict'
const readPkgUp = require('read-pkg-up')
const resolve = require('path').resolve
const dirname = require('path').dirname
const thenify = require('thenify')
const lock = thenify(require('lockfile').lock)
const unlock = thenify(require('lockfile').unlock)
const requireJson = require('../fs/require_json')
const writeJson = require('../fs/write_json')
const expandTilde = require('../fs/expand_tilde')
const resolveGlobalPkgPath = require('../resolve_global_pkg_path')

const logger = require('../logger')
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
    cmd.storeJson = storeJsonController(cmd.ctx.store)
    Object.assign(cmd.ctx, cmd.storeJson.read())
    if (!opts.quiet) cmd.ctx.log = logger(opts.logger)
    else cmd.ctx.log = () => () => {}

    function resolveStorePath (storePath) {
      if (storePath.indexOf('~/') === 0) {
        return expandTilde(storePath)
      }
      return resolve(root, storePath)
    }
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
