import {
  installCheckLogger,
  skippedOptionalDependencyLogger,
} from '@pnpm/core-loggers'
import { getSystemNodeVersion } from '@pnpm/env.system-node-version'
import { checkEngine, UnsupportedEngineError, type WantedEngine } from './checkEngine'
import { checkPlatform, UnsupportedPlatformError } from './checkPlatform'
import { type SupportedArchitectures } from '@pnpm/types'

export type { Engine } from './checkEngine'
export type { Platform, WantedPlatform } from './checkPlatform'

export {
  UnsupportedEngineError,
  UnsupportedPlatformError,
  type WantedEngine,
}

export function packageIsInstallable (
  pkgId: string,
  pkg: {
    name: string
    version: string
    engines?: WantedEngine
    cpu?: string[]
    os?: string[]
    libc?: string[]
  },
  options: {
    engineStrict?: boolean
    nodeVersion?: string
    optional: boolean
    pnpmVersion?: string
    lockfileDir: string
    supportedArchitectures?: SupportedArchitectures
  }
): boolean | null {
  const warn = checkPackage(pkgId, pkg, options)

  if (warn == null) return true

  installCheckLogger.warn({
    message: warn.message,
    prefix: options.lockfileDir,
  })

  if (options.optional) {
    skippedOptionalDependencyLogger.debug({
      details: warn.toString(),
      package: {
        id: pkgId,
        name: pkg.name,
        version: pkg.version,
      },
      prefix: options.lockfileDir,
      reason: warn.code === 'ERR_PNPM_UNSUPPORTED_ENGINE' ? 'unsupported_engine' : 'unsupported_platform',
    })

    return false
  }

  if (options.engineStrict) throw warn

  return null
}

export function checkPackage (
  pkgId: string,
  manifest: {
    engines?: WantedEngine
    cpu?: string[]
    os?: string[]
    libc?: string[]
  },
  options: {
    nodeVersion?: string
    pnpmVersion?: string
    supportedArchitectures?: SupportedArchitectures
  }
): null | UnsupportedEngineError | UnsupportedPlatformError {
  return checkPlatform(pkgId, {
    cpu: manifest.cpu ?? ['any'],
    os: manifest.os ?? ['any'],
    libc: manifest.libc ?? ['any'],
  }, options.supportedArchitectures) ?? (
    (manifest.engines == null)
      ? null
      : checkEngine(pkgId, manifest.engines, {
        node: options.nodeVersion ?? getSystemNodeVersion(),
        pnpm: options.pnpmVersion,
      })
  )
}
