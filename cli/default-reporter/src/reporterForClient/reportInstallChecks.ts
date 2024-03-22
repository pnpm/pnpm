import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'

import { autozoom } from './utils/zooming.js'
import { formatWarn } from './utils/formatWarn.js'

import type { InstallCheckLog } from '@pnpm/types'

export function reportInstallChecks(
  installCheck$: Rx.Observable<InstallCheckLog>,
  opts: {
    cwd: string
  }
): Rx.Observable<Rx.Observable<{
    msg: string;
  }>> {
  return installCheck$.pipe(
    map((log: InstallCheckLog): string | undefined => {
      return formatInstallCheck(opts.cwd, log);
    }),
    filter(Boolean),
    map((msg: string): Rx.Observable<{
      msg: string;
    }> => {
      return Rx.of({ msg });
    })
  )
}

function formatInstallCheck(
  currentPrefix: string,
  logObj: InstallCheckLog,
  opts?: {
    zoomOutCurrent: boolean
  } | undefined
): string | undefined {
  const zoomOutCurrent = opts?.zoomOutCurrent ?? false

  const prefix =
    'prefix' in logObj && typeof logObj.prefix === 'string'
      ? logObj.prefix
      : undefined

  switch (logObj.code) {
    case 'EBADPLATFORM': {
      return autozoom(
        currentPrefix,
        prefix,
        formatWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`),
        { zoomOutCurrent }
      )
    }

    case 'ENOTSUP': {
      return autozoom(currentPrefix, prefix, logObj.toString(), {
        zoomOutCurrent,
      })
    }

    default: {
      return undefined
    }
  }
}
