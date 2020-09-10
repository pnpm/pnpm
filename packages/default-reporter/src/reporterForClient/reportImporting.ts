import { ImportingLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map, scan } from 'rxjs/operators'

export default function (
  log$: {
    importing: Rx.Observable<ImportingLog>
  }
) {
  return log$.importing
    .pipe(
      scan((total) => total + 1, 0),
      map((total) => ({ msg: `Importing: ${total}` }))
    )
}
