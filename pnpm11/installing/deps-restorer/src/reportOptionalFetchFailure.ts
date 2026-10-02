import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import * as dp from '@pnpm/deps.path'
import { isError } from '@pnpm/error'
import type { DepPath } from '@pnpm/types'

export interface FailedOptionalPackage {
  depPath: DepPath
  name: string
  version: string
}

export function reportOptionalFetchFailure (err: unknown, pkg: FailedOptionalPackage, prefix: string): void {
  skippedOptionalDependencyLogger.debug({
    details: describeFetchError(err),
    package: {
      id: dp.removeSuffix(pkg.depPath),
      name: pkg.name,
      version: pkg.version,
    },
    prefix,
    reason: 'fetch_failure',
  })
}

function describeFetchError (err: unknown): string {
  if (!isError(err)) return String(err)
  const code = 'code' in err && typeof err.code === 'string' ? err.code : undefined
  return code ? `${code}: ${err.message}` : err.message
}
