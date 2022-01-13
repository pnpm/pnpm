import { DeprecationLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import chalk from 'chalk'
import formatWarn from './utils/formatWarn'
import { zoomOut } from './utils/zooming'

export default (
  deprecation$: Rx.Observable<DeprecationLog>,
  opts: {
    cwd: string
    isRecursive: boolean
  }
) => {
  return deprecation$.pipe(
    map((log) => {
      if (!opts.isRecursive && log.prefix === opts.cwd) {
        return Rx.of({
          msg: formatWarn(`${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}: ${log.deprecated}`),
        })
      }
      return Rx.of({
        msg: zoomOut(opts.cwd, log.prefix, formatWarn(`${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}`)),
      })
    })
  )
}
