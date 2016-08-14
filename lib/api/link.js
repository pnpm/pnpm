'use strict'
const path = require('path')
const readPkgUp = require('read-pkg-up')
const relSymlink = require('../fs/rel_symlink')
const installPkgDeps = require('./install_pkg_deps')
const resolveGlobalPkgPath = require('../resolve_global_pkg_path')
const linkPkgBins = require('../install/link_bins').linkPkgBins
const mkdirp = require('../fs/mkdirp')

module.exports = { linkFromRelative, linkFromGlobal, linkToGlobal }

function linkFromRelative (linkTo, opts) {
  opts = opts || {}
  const cwd = opts.cwd || process.cwd()
  const linkedPkgPath = path.resolve(cwd, linkTo)
  const currentModules = path.resolve(cwd, 'node_modules')
  return installPkgDeps(Object.assign({}, opts, { cwd: linkedPkgPath }))
    .then(_ => mkdirp(currentModules))
    .then(_ => readPkgUp({ cwd: linkedPkgPath }))
    .then(pkg => relSymlink(linkedPkgPath, path.resolve(currentModules, pkg.pkg.name)))
    .then(_ => linkPkgBins(currentModules, linkedPkgPath))
}

function linkFromGlobal (pkgName, opts) {
  const globalPkgPath = resolveGlobalPkgPath(opts.globalPath)
  const linkedPkgPath = path.join(globalPkgPath, 'node_modules', pkgName)
  return linkFromRelative(linkedPkgPath, opts)
}

function linkToGlobal (opts) {
  const globalPkgPath = resolveGlobalPkgPath(opts.globalPath)
  const cwd = opts.cwd || process.cwd()
  return linkFromRelative(cwd, Object.assign({
    cwd: globalPkgPath
  }, opts))
}
