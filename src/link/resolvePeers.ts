import path = require('path')
import semver = require('semver')
import {LinkedPackagesMap} from '.'
import logger from 'pnpm-logger'
import R = require('ramda')

type PackageVersions = {
  [version: string]: string
}

type InstalledPackageVersions = {
  [pkgName: string]: PackageVersions
}

export default async function linkPeers (pkgs: LinkedPackagesMap): Promise<LinkedPackagesMap> {
  const groupedPkgs: InstalledPackageVersions = {}

  R.values(pkgs).forEach(pkgData => {
    if (!pkgData.pkg.version) return

    const pkgName = pkgData.pkg.name
    groupedPkgs[pkgName] = groupedPkgs[pkgName] || {}
    groupedPkgs[pkgName][pkgData.pkg.version] = pkgData.id
  })

  await Promise.all(R.values(pkgs).map(async pkgData => {
    const peerDependencies = pkgData.pkg.peerDependencies || {}
    await Promise.all(Object.keys(peerDependencies).map(async peerName => {
      const version = semver.maxSatisfying(Object.keys(groupedPkgs[peerName] || {}), peerDependencies[peerName], true)
      if (!version) {
        logger.warn(`${pkgData.id} requires a peer of ${peerName}@${peerDependencies[peerName]} but none was installed.`)
        return
      }
      pkgData.dependencies.push(groupedPkgs[peerName][version])
    }))
  }))

  return pkgs
}
