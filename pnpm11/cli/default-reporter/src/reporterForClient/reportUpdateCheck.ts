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
  // `pnpm self-update` replaces the pnpm that PNPM_HOME manages. Corepack
  // refuses it outright, and an install another package manager owns is
  // resolved from that manager's bin directory rather than pnpm's home, so a
  // self-update would land beside the executable in use instead of replacing
  // it. The installer is the command that updates either one.
  if (isExecutedByCorepack(opts.env) || !opts.env.PNPM_HOME) {
    return standaloneInstallCommand(opts.platform)
  }
  return 'pnpm self-update'
}
