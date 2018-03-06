import logger from '@pnpm/logger'
import {PackageManifest} from '@pnpm/types'
import installChecks = require('pnpm-install-checks')
import R = require('ramda')
import {PkgByPkgId} from '../api/install'
import {installCheckLogger} from '../loggers'
import {splitNodeId} from '../nodeIdUtils'

export default async function getIsInstallable (
  pkgId: string,
  pkg: PackageManifest,
  options: {
    engineStrict: boolean,
    pkgByPkgId: PkgByPkgId,
    nodeId: string,
    nodeVersion: string,
    optional: boolean,
    pnpmVersion: string,
  },
): Promise<boolean> {
  const warn = await installChecks.checkPlatform(pkg) || await installChecks.checkEngine(pkg, {
    nodeVersion: options.nodeVersion,
    pnpmVersion: options.pnpmVersion,
  })

  if (!warn) return true

  installCheckLogger.warn(warn)

  if (options.optional) {
    const friendlyPath = nodeIdToFriendlyPath(options.nodeId, options.pkgByPkgId)
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
  pkgByPkgId: PkgByPkgId,
) {
  const pkgIds = splitNodeId(nodeId).slice(2, -2)
  return pkgIds
    .map((pkgId) => pkgByPkgId[pkgId].name)
    .join(' > ')
}
