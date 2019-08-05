import logger from '@pnpm/logger'
import { checkPackage, UnsupportedEngineError } from '@pnpm/package-is-installable'
import packageManager from './pnpmPkgJson'

export default function (
  pkgPath: string,
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
  opts: {
    engineStrict?: boolean,
  },
) {
  const err = checkPackage(pkgPath, pkg, {
    pnpmVersion: packageManager.stableVersion,
  })
  if (err === null) return
  if (
    (err instanceof UnsupportedEngineError && err.wanted.pnpm) ||
    opts.engineStrict
  ) throw err
  logger.warn({
    message: `Unsupported ${
      err instanceof UnsupportedEngineError ? 'engine' : 'platform'
    }: wanted: ${JSON.stringify(err.wanted)} (current: ${JSON.stringify(err.current)})`,
    prefix: pkgPath,
  })
}
