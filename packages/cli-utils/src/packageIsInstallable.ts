import packageManager from '@pnpm/cli-meta'
import logger from '@pnpm/logger'
import { checkPackage, UnsupportedEngineError, WantedEngine } from '@pnpm/package-is-installable'

export function packageIsInstallable (
  pkgPath: string,
  pkg: {
    engines?: WantedEngine
    cpu?: string[]
    os?: string[]
    libc?: string[]
  },
  opts: {
    engineStrict?: boolean
    nodeVersion?: string
  }
) {
  const pnpmVersion = packageManager.name === 'pnpm'
    ? packageManager.stableVersion
    : undefined
  const err = checkPackage(pkgPath, pkg, {
    nodeVersion: opts.nodeVersion,
    pnpmVersion,
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
