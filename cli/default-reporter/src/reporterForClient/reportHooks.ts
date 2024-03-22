import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import chalk from 'chalk'

import { autozoom } from './utils/zooming.js'

import type { HookLog } from '@pnpm/types'

export function reportHooks(
  hook$: Rx.Observable<HookLog>,
  opts: {
    cwd: string
    isRecursive: boolean
  }
): Rx.Observable<Rx.Observable<{
    msg: string;
  }>> {
  return hook$.pipe(
    map((log: HookLog): Rx.Observable<{
      msg: string;
    }> => {
      return Rx.of({
        msg: autozoom(
          opts.cwd,
          log.prefix,
          `${chalk.magentaBright(log.hook)}: ${log.message}`,
          {
            zoomOutCurrent: opts.isRecursive,
          }
        ),
      });
    }
    )
  )
}
