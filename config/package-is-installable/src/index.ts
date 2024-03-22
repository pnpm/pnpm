import '@total-typescript/ts-reset'

import type { ProjectManifest, SupportedArchitectures } from '@pnpm/types'
import { installCheckLogger, skippedOptionalDependencyLogger } from '@pnpm/core-loggers'

import { getSystemNodeVersion } from './getSystemNodeVersion.js'
import { checkEngine, UnsupportedEngineError } from './checkEngine.js'
import { checkPlatform, UnsupportedPlatformError } from './checkPlatform.js'

export { UnsupportedEngineError, UnsupportedPlatformError }

export function packageIsInstallable(
  pkgId: string,
  pkg: ProjectManifest,
  options: {
    engineStrict?: boolean | undefined
    nodeVersion?: string | undefined
    optional: boolean
    pnpmVersion?: string | undefined
    lockfileDir: string
    supportedArchitectures?: SupportedArchitectures | undefined
  }
): boolean | null {
  const warn = checkPackage(pkgId, pkg, options)

  if (warn == null) {
    return true
  }

  installCheckLogger.warn({
    message: warn.message,
    prefix: options.lockfileDir,
  })

  if (options.optional) {
    skippedOptionalDependencyLogger.debug({
      details: warn.toString(),
      package: {
        id: pkgId,
        name: pkg.name ?? '',
        version: pkg.version ?? '',
      },
      prefix: options.lockfileDir,
      reason:
        warn.code === 'ERR_PNPM_UNSUPPORTED_ENGINE'
          ? 'unsupported_engine'
          : 'unsupported_platform',
    })

    return false
  }

  if (options.engineStrict) {
    throw warn
  }

  return null
}

export function checkPackage(
  pkgId: string,
  manifest: ProjectManifest | undefined,
  options: {
    nodeVersion?: string | undefined
    pnpmVersion?: string | undefined
    supportedArchitectures?: SupportedArchitectures | undefined
  }
): null | UnsupportedEngineError | UnsupportedPlatformError {
  return (
    checkPlatform(
      pkgId,
      {
        cpu: manifest?.cpu ?? ['any'],
        os: manifest?.os ?? ['any'],
        libc: manifest?.libc ?? ['any'],
      },
      options.supportedArchitectures
    ) ??
    (manifest?.engines == null
      ? null
      : checkEngine(pkgId, manifest?.engines, {
        node: options.nodeVersion ?? getSystemNodeVersion(),
        pnpm: options.pnpmVersion,
      }))
  )
}
