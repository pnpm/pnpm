import { packageManager } from '@pnpm/cli.meta'
import { checkPackage, UnsupportedEngineError, type WantedEngine } from '@pnpm/config.package-is-installable'
import { logger } from '@pnpm/logger'
import type { SupportedArchitectures } from '@pnpm/types'

export function packageIsInstallable (
  pkgPath: string,
  pkg: {
    packageManager?: string
    engines?: WantedEngine
    cpu?: string[]
    os?: string[]
    libc?: string[]
  },
  opts: {
    engineStrict?: boolean
    nodeVersion?: string
    supportedArchitectures?: SupportedArchitectures
  }
): void {
  const currentPnpmVersion = packageManager.name === 'pnpm'
    ? packageManager.version
    : undefined
  const checkOptions = {
    nodeVersion: opts.nodeVersion,
    pnpmVersion: currentPnpmVersion,
    supportedArchitectures: opts.supportedArchitectures ?? {
      os: ['current'],
      cpu: ['current'],
      libc: ['current'],
    },
  }
  // The engines and the platform are checked apart, so that a platform
  // mismatch, which only warns, does not hide an engine failure.
  const { engines, ...platform } = pkg
  for (const part of [{ engines }, platform]) {
    const err = checkPackage(pkgPath, part, checkOptions)
    if (err === null) continue
    if (
      (err instanceof UnsupportedEngineError && err.wanted.pnpm) ??
      opts.engineStrict
    ) throw err
    logger.warn({
      message: `Unsupported ${
        err instanceof UnsupportedEngineError ? 'engine' : 'platform'
      }: wanted: ${JSON.stringify(err.wanted)} (current: ${JSON.stringify(err.current)})`,
      prefix: pkgPath,
    })
  }
}
