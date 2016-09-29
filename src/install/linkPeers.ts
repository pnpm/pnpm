import mkdirp from '../fs/mkdirp'
import unsymlink from '../fs/unsymlink'
import relSymlink from '../fs/relSymlink'
import path = require('path')
import semver = require('semver')
import {InstalledPackages} from '../api/install'
import {Package} from '../types'
import {InstalledPackage} from '.'

type Dict<T> = {
  [index: string]: T
}

/*
 * Links into `.store/node_modules`
 */
export default async function linkPeers (store: string, installs: InstalledPackages) {
  if (!installs) return
  const peers: Dict<InstalledPackage> = {}
  const roots: Dict<Package> = {}

  Object.keys(installs).forEach(name => {
    const pkgData = installs[name]
    const realname = pkgData.pkg.name

    if (pkgData.keypath.length === 0) {
      roots[realname] = pkgData.pkg
      return
    }

    // NOTE: version is not always available
    // version is guaranteed to be there only for packages loaded from the npm registry
    if (!peers[realname] || peers[realname].pkg.version && pkgData.pkg.version &&
      semver.gt(pkgData.pkg.version, peers[realname].pkg.version, true)) {
      peers[realname] = pkgData
    }
  })

  const modules = path.join(store, 'node_modules')
  await mkdirp(modules)
  await Promise.all(Object.keys(roots).map(name => {
    return unsymlink(path.join(modules, roots[name].name))
  }))

  await Promise.all(Object.keys(peers).map(async function (name) {
    await unsymlink(path.join(modules, peers[name].escapedName))
    return relSymlink(
      path.join(store, peers[name].id, '_'),
      path.join(modules, peers[name].escapedName))
  }))
}
