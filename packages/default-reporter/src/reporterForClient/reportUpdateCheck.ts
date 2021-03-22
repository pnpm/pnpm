import { UpdateCheckLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'
import semver from 'semver'

export default (log$: Rx.Observable<UpdateCheckLog>) => {
  return log$.pipe(
    filter((log) => semver.gt(log.latestVersion, log.currentVersion)),
    map((log) => Rx.of({
      msg: `\
~
~ Update available! ${chalk.red(log.currentVersion)} → ${chalk.green(log.latestVersion)}.
~ Run ${chalk.magenta('pnpm add -g pnpm')} to update.
~`,
    }))
  )
}
