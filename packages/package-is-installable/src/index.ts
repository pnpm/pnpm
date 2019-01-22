import {
  installCheckLogger,
  skippedOptionalDependencyLogger,
} from '@pnpm/core-loggers'
import checkEngine from './checkEngine'
import checkPlatform from './checkPlatform'

export default function packageIsInstallable (
  pkgId: string,
  pkg: {
    name: string,
    version: string,
    engines?: {
      node?: string,
      npm?: string,
    },
    cpu?: string[],
    os?: string[],
  },
  options: {
    engineStrict: boolean,
    nodeVersion: string,
    optional: boolean,
    pnpmVersion: string,
    prefix: string,
  },
): boolean | null {
  const warn = checkPlatform(pkgId, {
    cpu: pkg.cpu || ['any'],
    os: pkg.os || ['any'],
  })
  || pkg.engines && checkEngine(pkgId, pkg.engines, {
    node: options.nodeVersion,
    pnpm: options.pnpmVersion,
  })

  if (!warn) return true

  installCheckLogger.warn({
    message: warn.message,
    prefix: options.prefix,
  })

  if (options.optional) {
    skippedOptionalDependencyLogger.debug({
      details: warn.toString(),
      package: {
        id: pkgId,
        name: pkg.name,
        version: pkg.version,
      },
      prefix: options.prefix,
      reason: warn.code === 'ERR_PNPM_UNSUPPORTED_ENGINE' ? 'unsupported_engine' : 'unsupported_platform',
    })

    return false
  }

  if (options.engineStrict) throw warn

  return null
}
