import {
  installCheckLogger,
  skippedOptionalDependencyLogger,
} from '@pnpm/core-loggers'
import { getSystemNodeVersion } from '@pnpm/engine.runtime.system-version'
import type { SupportedArchitectures } from '@pnpm/types'

import { checkEngine, UnsupportedEngineError, type WantedEngine } from './checkEngine.js'
import { checkPlatform, UnsupportedPlatformError } from './checkPlatform.js'
import { inferPlatformFromPackageName } from './inferPlatformFromPackageName.js'

export type { Engine } from './checkEngine.js'
export type { Platform, WantedPlatform } from './checkPlatform.js'
export { inferPlatformFromPackageName } from './inferPlatformFromPackageName.js'

export {
  UnsupportedEngineError,
  UnsupportedPlatformError,
  type WantedEngine,
}

export interface InstallabilityManifest {
  name: string
  version: string
  engines?: WantedEngine
  cpu?: string[]
  os?: string[]
  libc?: string[]
}

/**
 * The installability verdict on its own — `null` when the package is
 * installable — without the logging and the `engineStrict` throw that
 * [packageIsInstallable] wraps around it. Callers that have to classify a
 * package before they know how to report it use this, so the report is
 * emitted exactly once.
 */
export function checkPackageInstallability (
  pkgId: string,
  pkg: InstallabilityManifest,
  options: {
    nodeVersion?: string
    optional: boolean
    pnpmVersion?: string
    supportedArchitectures?: SupportedArchitectures
  }
): null | UnsupportedEngineError | UnsupportedPlatformError {
  return checkPackage(pkgId, { engines: pkg.engines, ...effectivePlatform(pkg, options.optional) }, options)
}

export function packageIsInstallable (
  pkgId: string,
  pkg: InstallabilityManifest,
  options: {
    engineStrict?: boolean
    nodeVersion?: string
    optional: boolean
    pnpmVersion?: string
    lockfileDir: string
    supportedArchitectures?: SupportedArchitectures
  }
): boolean | null {
  const warn = checkPackageInstallability(pkgId, pkg, options)

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

interface PlatformFields {
  cpu?: string[]
  os?: string[]
  libc?: string[]
}

/**
 * The platform fields of an optional dependency may be incomplete: some
 * registries strip os/cpu/libc (or just libc) from the metadata they serve,
 * and lockfile entries written from such metadata lack them too. For a
 * platform-specific binary the package name carries the same information, so
 * each missing field is filled from the name's tokens. A package that
 * declares no platform fields at all is treated as platform-specific only
 * when an operating system is recognized in its name — a generic name
 * segment (e.g. `arm` on its own) never marks it as such.
 * https://github.com/pnpm/pnpm/issues/11702
 */
function effectivePlatform (pkg: PlatformFields & { name: string }, optional: boolean): PlatformFields {
  if (!optional || (pkg.os != null && pkg.cpu != null && pkg.libc != null)) return pkg
  const inferred = inferPlatformFromPackageName(pkg.name)
  if (inferred == null) return pkg
  const pkgDeclaresPlatform = pkg.os != null || pkg.cpu != null || pkg.libc != null
  if (!pkgDeclaresPlatform && inferred.os == null) return pkg
  return {
    os: pkg.os ?? inferred.os,
    cpu: pkg.cpu ?? inferred.cpu,
    libc: pkg.libc ?? inferred.libc,
  }
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
        node: options.nodeVersion ?? getSystemNodeVersion() ?? process.version,
        pnpm: options.pnpmVersion,
      })
  )
}
