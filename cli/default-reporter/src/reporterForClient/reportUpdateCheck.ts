import { type UpdateCheckLog } from '@pnpm/core-loggers'
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
${chalk.magenta('Changelog:')} https://github.com/pnpm/pnpm/releases/tag/v${log.latestVersion}
${updateMessage}

Follow ${chalk.magenta('@pnpmjs')} for updates: https://twitter.com/pnpmjs`,
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
  if (opts.currentPkgIsExecutable && opts.env.PNPM_HOME) {
    return 'Run a script from: https://pnpm.io/installation'
  }
  const updateCommand = renderUpdateCommand(opts)
  return `Run "${chalk.magenta(updateCommand)}" to update.`
}

function renderUpdateCommand (opts: UpdateMessageOptions): string {
  if (opts.env.COREPACK_ROOT) {
    return `corepack install -g pnpm@${opts.latestVersion}`
  }
  const pkgName = opts.currentPkgIsExecutable ? '@pnpm/exe' : 'pnpm'
  return `pnpm add -g ${pkgName}`
}

function detectIfCurrentPkgIsExecutable (process: NodeJS.Process): boolean {
  return process['pkg'] != null
}
