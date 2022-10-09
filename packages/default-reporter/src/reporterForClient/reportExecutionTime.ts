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
        msg: `Done in ${prettyMs(log.endedAt - log.startedAt)}`,
      })
    })
  )
}
