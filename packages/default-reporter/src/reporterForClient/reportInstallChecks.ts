import { InstallCheckLog } from '@pnpm/core-loggers'
import most = require('most')
import formatWarn from './utils/formatWarn'
import { autozoom } from './utils/zooming'

export default (
  installCheck$: most.Stream<InstallCheckLog>,
  opts: {
    cwd: string,
  }
) => {
  return installCheck$
    .map(formatInstallCheck.bind(null, opts.cwd))
    .filter(Boolean)
    .map((msg) => ({ msg }))
    .map(most.of) as most.Stream<most.Stream<{msg: string}>>
}

function formatInstallCheck (
  currentPrefix: string,
  logObj: InstallCheckLog,
  opts?: {
    zoomOutCurrent: boolean,
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
      return
  }
}
