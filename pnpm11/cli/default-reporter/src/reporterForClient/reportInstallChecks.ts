import type { InstallCheckLog } from '@pnpm/core-loggers'
import type { LogBaseWarn } from '@pnpm/logger'
import { sanitizeInline } from '@pnpm/text.sanitize'
import * as Rx from 'rxjs'
import { distinct, filter, map } from 'rxjs/operators'

import { formatWarn } from './utils/formatWarn.js'
import { autozoom } from './utils/zooming.js'

export type InstallCheckEvent = InstallCheckLog | ({ name: 'pnpm:install-check' } & LogBaseWarn)

export function reportInstallChecks (
  installCheck$: Rx.Observable<InstallCheckEvent>,
  opts: {
    cwd: string
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return installCheck$.pipe(
    map((log) => formatInstallCheck(opts.cwd, log)),
    filter(Boolean),
    // Several install stages check the same package.
    distinct(),
    map((msg) => Rx.of({ msg }))
  )
}

function formatInstallCheck (
  currentPrefix: string,
  logObj: InstallCheckEvent,
  opts?: {
    zoomOutCurrent: boolean
  }
): string | undefined {
  const zoomOutCurrent = opts?.zoomOutCurrent ?? false
  if (typeof logObj.message === 'string') {
    return autozoom(
      currentPrefix,
      logObj.prefix,
      // The message names a package from the lockfile, which a contributor can edit.
      formatWarn(sanitizeInline(logObj.message)),
      { zoomOutCurrent }
    )
  }
  if (!('code' in logObj)) return undefined
  switch (logObj.code) {
    case 'EBADPLATFORM':
      return autozoom(
        currentPrefix,
        logObj.prefix,
        formatWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`),
        { zoomOutCurrent }
      )
    case 'ENOTSUP':
      return autozoom(currentPrefix, logObj.prefix, logObj.toString(), { zoomOutCurrent })
    default:
      return undefined
  }
}
