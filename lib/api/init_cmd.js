'use strict'
var readPkgUp = require('read-pkg-up')
var resolve = require('path').resolve
var dirname = require('path').dirname
var thenify = require('thenify')
var lock = thenify(require('lockfile').lock)
var unlock = thenify(require('lockfile').unlock)
var requireJson = require('../fs/require_json')
var writeJson = require('../fs/write_json')
var expandTilde = require('../fs/expand_tilde')

var logger = require('../logger')
var storeJsonController = require('../fs/store_json_controller')
var mkdirp = require('../fs/mkdirp')

module.exports = function (opts) {
  opts = opts || {}
  const cwd = opts.cwd || process.cwd()
  var cmd = {
    ctx: {}
  }
  var lockfile
  return (opts.global ? readGlobalPkg(opts.globalPath) : readPkgUp({ cwd }))
    .then(_ => { cmd.pkg = _ })
    .then(_ => updateContext())
    .then(_ => mkdirp(cmd.ctx.store))
    .then(_ => lock(lockfile))
    .then(_ => cmd)

  function updateContext () {
    var root = cmd.pkg.path ? dirname(cmd.pkg.path) : cwd
    cmd.ctx.root = root
    cmd.ctx.store = resolveStorePath(opts.storePath)
    lockfile = resolve(cmd.ctx.store, 'lock')
    cmd.unlock = () => unlock(lockfile)
    cmd.storeJson = storeJsonController(cmd.ctx.store)
    Object.assign(cmd.ctx, cmd.storeJson.read())
    if (!opts.quiet) cmd.ctx.log = logger(opts.logger)
    else cmd.ctx.log = function () { return function () {} }

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
  const globalPnpm = expandTilde(globalPath)
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
