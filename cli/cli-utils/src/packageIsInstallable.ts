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
    packageManagerStrictVersion?: boolean
    engineStrict?: boolean
    nodeVersion?: string
    supportedArchitectures?: SupportedArchitectures
  }
): void {
  const currentPnpmVersion = packageManager.name === 'pnpm'
    ? packageManager.version
    : undefined
  if (pkg.packageManager && !process.env.COREPACK_ROOT) {
    const [pmName, pmReference] = pkg.packageManager.split('@')
    if (pmName && pmName !== 'pnpm') {
      const msg = `This project is configured to use ${pmName}`
      if (opts.packageManagerStrict) {
        throw new PnpmError('OTHER_PM_EXPECTED', msg)
      } else {
        globalWarn(msg)
      }
    } else if (currentPnpmVersion && opts.packageManagerStrictVersion && !pmReference.includes(':')) {
      // pmReference is semantic versioning, not URL
      const [requiredPnpmVersion] = pmReference.split('+')
      if (requiredPnpmVersion && requiredPnpmVersion !== currentPnpmVersion) {
        const msg = `This project is configured to use v${requiredPnpmVersion} of pnpm. Your current pnpm is v${currentPnpmVersion}`
        if (opts.packageManagerStrict) {
          throw new PnpmError('BAD_PM_VERSION', msg, {
            hint: 'If you want to bypass this version check, you can set the "package-manager-strict" configuration to "false" or set the "COREPACK_ENABLE_STRICT" environment variable to "0"',
          })
        } else {
          globalWarn(msg)
        }
      }
    }
  }
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
