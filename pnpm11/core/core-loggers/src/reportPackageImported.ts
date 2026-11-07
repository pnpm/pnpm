import { packageImportMethodLogger, type PackageImportMethodMessage } from './packageImportMethodLogger.js'
import { progressLogger } from './progressLogger.js'

/**
 * Report one package materialized from the content-addressable store into
 * the virtual store.
 *
 * The import itself runs in a `@pnpm/worker` thread, whose loggers are not
 * connected to the reporter, so both channels are emitted here — in the main
 * process — from the method the worker reported back. `pnpm:progress` drives
 * the "imported" counter; `pnpm:package-import-method` tells the reporter
 * which wording to use for the store block it prints on a first install.
 */
export function reportPackageImported (
  opts: {
    method: string
    requester: string
    to: string
  }
): void {
  progressLogger.debug({
    method: opts.method,
    requester: opts.requester,
    status: 'imported',
    to: opts.to,
  })
  if (isKnownImportMethod(opts.method)) {
    packageImportMethodLogger.debug({ method: opts.method })
  }
}

function isKnownImportMethod (method: string): method is PackageImportMethodMessage['method'] {
  return method === 'clone' || method === 'hardlink' || method === 'copy'
}
