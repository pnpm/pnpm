import { InstallCheckLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'
import formatWarn from './utils/formatWarn'
import { autozoom } from './utils/zooming'

export default (
  installCheck$: Rx.Observable<InstallCheckLog>,
  opts: {
    cwd: string
  }
) => {
  return installCheck$.pipe(
    map((log) => formatInstallCheck(opts.cwd, log)),
    filter(Boolean),
    map((msg) => Rx.of({ msg }))
  )
}

function formatInstallCheck (
  currentPrefix: string,
  logObj: InstallCheckLog,
  opts?: {
    zoomOutCurrent: boolean
  }
) {
  const zoomOutCurrent = opts?.zoomOutCurrent ?? false
  switch (logObj.code) {
  case 'EBADPLATFORM':
    return autozoom(
      currentPrefix,
      logObj['prefix'],
      formatWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`),
      { zoomOutCurrent }
    )
  case 'ENOTSUP':
    return autozoom(currentPrefix, logObj['prefix'], logObj.toString(), { zoomOutCurrent })
  default:
    return undefined
  }
}
