import mkdirp from '../fs/mkdirp'
import unsymlink from '../fs/unsymlink'
import relSymlink from '../fs/rel_symlink'
import path = require('path')
import semver = require('semver')

/*
 * Links into `.store/node_modules`
 */

export default function linkPeers (pkg, store, installs) {
  if (!installs) return
  const peers = {}
  const roots = {}

  Object.keys(installs).forEach(name => {
    const pkgData = installs[name]
    const realname = pkgData.name

    if (pkgData.keypath.length === 0) {
      roots[realname] = pkgData
      return
    }

    // NOTE: version is not always available
    // version is guaranteed to be there only for packages loaded from the npm registry
    if (!peers[realname] || peers[realname].version && pkgData.version &&
      semver.gt(pkgData.version, peers[realname].version)) {
      peers[realname] = pkgData
    }
  })

  const modules = path.join(store, 'node_modules')
  return mkdirp(modules)
    .then(_ => Promise.all(Object.keys(roots).map(name => {
      return unsymlink(path.join(modules, roots[name].name))
    })))
    .then(_ => Promise.all(Object.keys(peers).map(name =>
      unsymlink(path.join(modules, peers[name].spec.escapedName))
      .then(_ =>
        relSymlink(
          path.join(store, peers[name].fullname, '_'),
          path.join(modules, peers[name].spec.escapedName)))
    )))
}
