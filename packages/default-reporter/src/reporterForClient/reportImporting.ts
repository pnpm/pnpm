import { ImportingLog, StageLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map, scan } from 'rxjs/operators'
import { hlValue } from './outputConstants'

export default function (
  log$: {
    importing: Rx.Observable<ImportingLog>
    stage: Rx.Observable<StageLog>
  },
  opts: {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    throttle?: Rx.OperatorFunction<any, any>
  }
) {
  let importedTotal = log$.importing.pipe(
    scan((total) => total + 1, 0)
  )
  if (opts.throttle) {
    importedTotal = importedTotal.pipe(opts.throttle)
  }
  return Rx.combineLatest(importedTotal, log$.stage)
    .pipe(
      map(([total]) => ({
        fixed: true,
        msg: `Adding packages to the virtual store: ${hlValue(total)}`,
      }))
    )
}
