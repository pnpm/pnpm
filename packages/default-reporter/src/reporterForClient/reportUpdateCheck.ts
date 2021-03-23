import { UpdateCheckLog } from '@pnpm/core-loggers'
import boxen from 'boxen'
import chalk from 'chalk'
import * as Rx from 'rxjs'
import { filter, map, take } from 'rxjs/operators'
import semver from 'semver'

export default (log$: Rx.Observable<UpdateCheckLog>) => {
  return log$.pipe(
    take(1),
    filter((log) => semver.gt(log.latestVersion, log.currentVersion)),
    map((log) => Rx.of({
      msg: boxen(`\
Update available! ${chalk.red(log.currentVersion)} → ${chalk.green(log.latestVersion)}.
${chalk.magenta('Changelog:')} https://github.com/pnpm/pnpm/releases/tag/v${log.latestVersion}
Run ${chalk.magenta('pnpm add -g pnpm')} to update.

Follow ${chalk.magenta('@pnpmjs')} for updates: https://twitter.com/pnpmjs`,
      {
        padding: 1,
        margin: 1,
        align: 'center',
        borderColor: 'yellow',
        borderStyle: 'round',
      }
      ),
    }))
  )
}
