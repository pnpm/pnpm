import prettyMs from 'pretty-ms'
import { FinishTimeLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'

export default (
  finishTime$: Rx.Observable<FinishTimeLog>
) => {
  return finishTime$.pipe(
    take(1),
    map((log) => {
      return Rx.of({
        msg: `Done in ${prettyMs(log.finishedAt - log.startedAt)}`,
      })
    })
  )
}
