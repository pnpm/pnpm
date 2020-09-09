import { DeprecationLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'
import formatWarn from './utils/formatWarn'
import { zoomOut } from './utils/zooming'
import chalk = require('chalk')

export default (
  deprecation$: Rx.Observable<DeprecationLog>,
  opts: {
    cwd: string
    isRecursive: boolean
  }
) => {
  return deprecation$.pipe(
    // print warnings only about deprecated packages from the root
    filter((log) => log.depth === 0),
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
