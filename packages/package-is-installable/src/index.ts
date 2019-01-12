import {
  installCheckLogger,
  skippedOptionalDependencyLogger,
} from '@pnpm/core-loggers'
import { PackageManifest } from '@pnpm/types'
import installChecks = require('pnpm-install-checks')

export default async function getIsInstallable (
  pkgId: string,
  pkg: PackageManifest,
  options: {
    engineStrict: boolean,
    nodeId: string,
    nodeVersion: string,
    optional: boolean,
    pnpmVersion: string,
    prefix: string,
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
      parents: undefined, // TODO: remove this field from the type
      prefix: options.prefix,
      reason: warn.code === 'ENOTSUP' ? 'unsupported_engine' : 'unsupported_platform',
    })

    return false
  }

  if (options.engineStrict) throw warn

  return true
}
