import { PnpmError } from '@pnpm/error'
import { packageManager } from '@pnpm/cli-meta'
import { logger, globalWarn } from '@pnpm/logger'
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
    engineStrict?: boolean
    nodeVersion?: string
    supportedArchitectures?: SupportedArchitectures
  }
): void {
  const pnpmVersion = packageManager.name === 'pnpm'
    ? packageManager.version
    : undefined
  if (pkg.packageManager && !process.env.COREPACK_ROOT) {
    const [pmName] = pkg.packageManager.split('@')
    if (pmName && pmName !== 'pnpm') {
      const msg = `This project is configured to use ${pmName}`
      if (opts.packageManagerStrict) {
        throw new PnpmError('OTHER_PM_EXPECTED', msg)
      } else {
        globalWarn(msg)
      }
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
