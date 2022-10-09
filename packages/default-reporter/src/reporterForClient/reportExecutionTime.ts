import prettyMs from 'pretty-ms'
import { ExecutionTimeLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'

export function reportExecutionTime (
  executionTime$: Rx.Observable<ExecutionTimeLog>
) {
  return executionTime$.pipe(
    take(1),
    map((log) => {
      return Rx.of({
        fixed: true, // Without this, for some reason sometimes the progress bar is printed after the execution time
        msg: `Done in ${prettyMs(log.endedAt - log.startedAt)}`,
      })
    })
  )
}
