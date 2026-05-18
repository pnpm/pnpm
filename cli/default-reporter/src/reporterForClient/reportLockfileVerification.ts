import type { LockfileVerificationLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import prettyMs from 'pretty-ms'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'

export function reportLockfileVerification (
  lockfileVerification$: Rx.Observable<LockfileVerificationLog>
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  // A single inner observable so the `done` message overwrites the
  // transient `started` message in ansi-diff mode. In appendOnly mode
  // both lines are printed.
  return Rx.of(lockfileVerification$.pipe(
    map((log) => {
      switch (log.status) {
        case 'started':
          return {
            msg: `${chalk.cyan('?')} Verifying lockfile (${log.entries} ${log.entries === 1 ? 'entry' : 'entries'})...`,
          }
        case 'done':
          return {
            msg: `${chalk.green('✓')} Lockfile verified (${log.entries} ${log.entries === 1 ? 'entry' : 'entries'} in ${prettyMs(log.elapsedMs)})`,
          }
      }
    })
  ))
}
