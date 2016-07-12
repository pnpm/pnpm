var Promise = require('../promise')
var mkdirp = require('../fs/mkdirp')
var unsymlink = require('../fs/unsymlink')
var relSymlink = require('../fs/rel_symlink')
var join = require('path').join
var semver = require('semver')

/*
 * Links into `.store/node_modules`
 */

module.exports = function linkPeers (pkg, store, installs) {
  var peers = {}
  var roots = {}

  Object.keys(installs).forEach(name => {
    var pkgData = installs[name]
    var realname = pkgData.name

    if (pkgData.keypath.length === 0) {
      roots[realname] = pkgData
    } else if (!peers[realname] ||
      semver.gt(pkgData.version, peers[realname].version)) {
      peers[realname] = pkgData
    }
  })

  var modules = join(store, 'node_modules')
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
