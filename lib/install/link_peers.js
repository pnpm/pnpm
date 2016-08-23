'use strict'
const mkdirp = require('../fs/mkdirp')
const unsymlink = require('../fs/unsymlink')
const relSymlink = require('../fs/rel_symlink')
const join = require('path').join
const semver = require('semver')

/*
 * Links into `.store/node_modules`
 */

module.exports = function linkPeers (pkg, store, installs) {
  if (!installs) return
  const peers = {}
  const roots = {}

  Object.keys(installs).forEach(name => {
    const pkgData = installs[name]
    const realname = pkgData.name

    if (pkgData.keypath.length === 0) {
      roots[realname] = pkgData
    } else if (!peers[realname] ||
      semver.gt(pkgData.version, peers[realname].version)) {
      peers[realname] = pkgData
    }
  })

  const modules = join(store, 'node_modules')
  return mkdirp(modules)
    .then(_ => Promise.all(Object.keys(roots).map(name => {
      return unsymlink(join(modules, roots[name].name))
    })))
    .then(_ => Promise.all(Object.keys(peers).map(name =>
      unsymlink(join(modules, peers[name].spec.escapedName))
      .then(_ =>
        relSymlink(
          join(store, peers[name].fullname, '_'),
          join(modules, peers[name].spec.escapedName)))
    )))
}
