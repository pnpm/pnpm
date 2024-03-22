import * as Rx from 'rxjs'
import prettyMs from 'pretty-ms'
import { map, take } from 'rxjs/operators'

import type { ExecutionTimeLog } from '@pnpm/types'

export function reportExecutionTime(
  executionTime$: Rx.Observable<ExecutionTimeLog>
): Rx.Observable<Rx.Observable<{
    fixed: boolean;
    msg: string;
  }>> {
  return executionTime$.pipe(
    take(1),
    map((log: ExecutionTimeLog): Rx.Observable<{
      fixed: true;
      msg: string;
    }> => {
      return Rx.of({
        fixed: true, // Without this, for some reason sometimes the progress bar is printed after the execution time
        msg: `Done in ${prettyMs(log.endedAt - log.startedAt)}`,
      })
    })
  )
}
