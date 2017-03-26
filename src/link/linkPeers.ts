import mkdirp = require('mkdirp-promise')
import symlinkDir from 'symlink-dir'
import path = require('path')
import semver = require('semver')
import {InstalledPackages} from '../api/install'
import {Package} from '../types'
import {LinkedPackage} from '.'
import logger from 'pnpm-logger'

type Dict<T> = {
  [index: string]: T
}

type PackageVersions = {
  [version: string]: LinkedPackage
}

type InstalledPackageVersions = {
  [pkgName: string]: PackageVersions
}

export default async function linkPeers (pkgs: LinkedPackage[]) {
  if (!pkgs) return

  const groupedPkgs: InstalledPackageVersions = {}

  pkgs.forEach(pkgData => {
    if (!pkgData.pkg.version) return

    const pkgName = pkgData.pkg.name
    groupedPkgs[pkgName] = groupedPkgs[pkgName] || {}
    groupedPkgs[pkgName][pkgData.pkg.version] = pkgData
  })

  return Promise.all(pkgs.map(pkgData => {
    const peerDependencies = pkgData.pkg.peerDependencies || {}
    return Promise.all(Object.keys(peerDependencies).map(peerName => {
      const version = semver.maxSatisfying(Object.keys(groupedPkgs[peerName] || {}), peerDependencies[peerName], true)
      if (!version) {
        logger.warn(`${pkgData.id} requires a peer of ${peerName}@${peerDependencies[peerName]} but none was installed.`)
        return
      }
      return symlinkDir(
        groupedPkgs[peerName][version].hardlinkedLocation,
        path.join(pkgData.modules, peerName)
      )
    }))
  }))
}
