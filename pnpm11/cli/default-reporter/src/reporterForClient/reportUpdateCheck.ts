import { isExecutedByCorepack, standaloneInstallCommand } from '@pnpm/cli.meta'
import type { UpdateCheckLog } from '@pnpm/core-loggers'
import boxen from 'boxen'
import chalk from 'chalk'
import * as Rx from 'rxjs'
import { filter, map, take } from 'rxjs/operators'
import semver from 'semver'

export function reportUpdateCheck (log$: Rx.Observable<UpdateCheckLog>, opts: {
  env: NodeJS.ProcessEnv
  process: NodeJS.Process
}): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return log$.pipe(
    take(1),
    filter((log) => semver.gt(log.latestVersion, log.currentVersion)),
    map((log) => {
      const updateMessage = renderUpdateMessage({
        env: opts.env,
        platform: opts.process.platform,
      })
      return Rx.of({
        msg: boxen(`\
Update available! ${chalk.red(log.currentVersion)} → ${chalk.green(log.latestVersion)}.
${chalk.magenta('Changelog:')} https://pnpm.io/v/${log.latestVersion}
${updateMessage}`,
        {
          padding: 1,
          margin: 1,
          align: 'center',
          borderColor: 'yellow',
          borderStyle: 'round',
        }
        ),
      })
    })
  )
}

interface UpdateMessageOptions {
  env: NodeJS.ProcessEnv
  platform: NodeJS.Platform
}

function renderUpdateMessage (opts: UpdateMessageOptions): string {
  const updateCommand = renderUpdateCommand(opts)
  return `To update, run: ${chalk.magenta(updateCommand)}`
}

function renderUpdateCommand (opts: UpdateMessageOptions): string {
  // Under Corepack, `pnpm self-update` refuses to run, and pnpm no longer
  // points users back at Corepack — it suggests its own installer instead.
  if (isExecutedByCorepack(opts.env)) {
    return standaloneInstallCommand(opts.platform)
  }
  // `pnpm add -g pnpm` (or `@pnpm/exe`) is refused by the add command itself,
  // which points at self-update instead. self-update also picks the package
  // that can actually deliver a working binary for the wanted version — the
  // unscoped `pnpm` from v12, where `@pnpm/exe` is no longer published.
  return 'pnpm self-update'
}
