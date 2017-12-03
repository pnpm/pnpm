import R = require('ramda')
import installChecks = require('pnpm-install-checks')
import logger from '@pnpm/logger'
import {PackageManifest} from '@pnpm/types'
import {installCheckLogger} from '../loggers'
import {InstalledPackages} from '../api/install'
import {FetchedPackage} from '@pnpm/package-requester'

export default async function getIsInstallable (
  pkgId: string,
  pkg: PackageManifest,
  fetchedPkg: FetchedPackage,
  options: {
    nodeId: string,
    installs: InstalledPackages,
    optional: boolean,
    engineStrict: boolean,
    nodeVersion: string,
    pnpmVersion: string,
  }
): Promise<boolean> {
  const warn = await installChecks.checkPlatform(pkg) || await installChecks.checkEngine(pkg, {
    pnpmVersion: options.pnpmVersion,
    nodeVersion: options.nodeVersion
  })

  if (!warn) return true

  installCheckLogger.warn(warn)

  if (options.optional) {
    const friendlyPath = nodeIdToFriendlyPath(options.nodeId, options.installs)
    logger.warn({
      message: `${friendlyPath ? `${friendlyPath}: ` : ''}Skipping failed optional dependency ${pkg.name}@${pkg.version}`,
      warn,
    })

    return false
  }

  if (options.engineStrict) throw warn

  return true
}

function nodeIdToFriendlyPath (
  nodeId: string,
  installs: InstalledPackages,
) {
  const pkgIds = nodeId.split(':').slice(2, -2)
  return pkgIds
    .map(pkgId => installs[pkgId].name)
    .join(' > ')
}
