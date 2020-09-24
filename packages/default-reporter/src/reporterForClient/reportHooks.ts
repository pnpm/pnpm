import { HookLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import { autozoom } from './utils/zooming'
import chalk = require('chalk')

export default (
  hook$: Rx.Observable<HookLog>,
  opts: {
    cwd: string
    isRecursive: boolean
  }
) => {
  return hook$.pipe(
    map((log) => Rx.of({
      msg: autozoom(
        opts.cwd,
        log.prefix,
        `${chalk.magentaBright(log.hook)}: ${log.message}`,
        {
          zoomOutCurrent: opts.isRecursive,
        }
      ),
    }))
  )
}
