import logger from '@pnpm/logger'
import {PackageManifest} from '@pnpm/types'
import installChecks = require('pnpm-install-checks')
import R = require('ramda')
import {PkgByPkgId} from '../api/install'
import {
  installCheckLogger,
  skippedOptionalDependencyLogger,
} from '../loggers'
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
  const warn = await installChecks.checkPlatform({
      _id: pkgId,
      cpu: pkg.cpu,
      os: pkg.os,
    }) ||
    await installChecks.checkEngine({
      _id: pkgId,
      engines: pkg.engines,
    }, {
      nodeVersion: options.nodeVersion,
      pnpmVersion: options.pnpmVersion,
    })

  if (!warn) return true

  installCheckLogger.warn(warn)

  if (options.optional) {
    skippedOptionalDependencyLogger.debug({
      details: warn.toString(),
      package: {
        id: pkgId,
        name: pkg.name,
        version: pkg.version,
      },
      parents: nodeIdToParents(options.nodeId, options.pkgByPkgId),
      reason: warn.code === 'ENOTSUP' ? 'unsupported_engine' : 'unsupported_platform',
    })

    return false
  }

  if (options.engineStrict) throw warn

  return true
}

export function nodeIdToParents (
  nodeId: string,
  pkgByPkgId: PkgByPkgId,
) {
  const pkgIds = splitNodeId(nodeId).slice(2, -2)
  return pkgIds
    .map((pkgId) => {
      const pkg = pkgByPkgId[pkgId]
      return {
        id: pkg.id,
        name: pkg.name,
        version: pkg.version,
      }
    })
}
