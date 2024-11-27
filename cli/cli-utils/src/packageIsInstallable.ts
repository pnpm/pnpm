import { packageManager } from '@pnpm/cli-meta'
import { logger } from '@pnpm/logger'
import { checkPackage, UnsupportedEngineError, type WantedEngine } from '@pnpm/package-is-installable'
import { type SupportedArchitectures } from '@pnpm/types'

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
    packageManagerStrict?: boolean
    packageManagerStrictVersion?: boolean
    engineStrict?: boolean
    nodeVersion?: string
    supportedArchitectures?: SupportedArchitectures
  }
): void {
  const currentPnpmVersion = packageManager.name === 'pnpm'
    ? packageManager.version
    : undefined
  const err = checkPackage(pkgPath, pkg, {
    nodeVersion: opts.nodeVersion,
    pnpmVersion: currentPnpmVersion,
    supportedArchitectures: opts.supportedArchitectures ?? {
      os: ['current'],
      cpu: ['current'],
      libc: ['current'],
    },
  })
  if (err === null) return
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
