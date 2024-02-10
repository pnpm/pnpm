import { PnpmError } from '@pnpm/error'
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
    engineStrict?: boolean
    nodeVersion?: string
    supportedArchitectures?: SupportedArchitectures
  }
) {
  const pnpmVersion = packageManager.name === 'pnpm'
    ? packageManager.stableVersion
    : undefined
  if (pkg.packageManager) {
    const [pmName, pmVersion] = pkg.packageManager.split('@')
    if (pmName && pmName !== 'pnpm') {
      throw new PnpmError('OTHER_PM_EXPECTED', `This project is configured to use ${pmName}`)
    }
    if (pmVersion && pnpmVersion && pmVersion !== pnpmVersion) {
      throw new PnpmError('BAD_PM_VERSION', `This project is configured to use v${pmVersion} of pnpm. Your current pnpm is v${pnpmVersion}`)
    }
  }
  const err = checkPackage(pkgPath, pkg, {
    nodeVersion: opts.nodeVersion,
    pnpmVersion,
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
