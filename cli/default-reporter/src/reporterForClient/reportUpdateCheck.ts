import { type UpdateCheckLog } from '@pnpm/core-loggers'
import { detectIfCurrentPkgIsExecutable, isExecutedByCorepack } from '@pnpm/cli-meta'
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
        currentPkgIsExecutable: detectIfCurrentPkgIsExecutable(opts.process),
        latestVersion: log.latestVersion,
        env: opts.env,
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
  currentPkgIsExecutable: boolean
  env: NodeJS.ProcessEnv
  latestVersion: string
}

function renderUpdateMessage (opts: UpdateMessageOptions): string {
  const updateCommand = renderUpdateCommand(opts)
  return `To update, run: ${chalk.magenta(updateCommand)}`
}

function renderUpdateCommand (opts: UpdateMessageOptions): string {
  if (isExecutedByCorepack(opts.env)) {
    return `corepack use pnpm@${opts.latestVersion}`
  }
  if (opts.env.PNPM_HOME) {
    return 'pnpm self-update'
  }
  const pkgName = opts.currentPkgIsExecutable ? '@pnpm/exe' : 'pnpm'
  return `pnpm add -g ${pkgName}`
}
