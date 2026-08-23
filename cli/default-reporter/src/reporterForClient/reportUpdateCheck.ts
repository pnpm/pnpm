import { type UpdateCheckLog } from '@pnpm/core-loggers'
import { detectIfCurrentPkgIsExecutable, isExecutedByCorepack, type Process } from '@pnpm/cli-meta'
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
        latestVersion: log.latestVersion,
        env: opts.env,
        proc: opts.process,
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
  latestVersion: string
  proc: Process
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
  return `pnpm add -g ${updatePkgName(opts)}`
}

/**
 * The package to install for an update to `latestVersion`, mirroring what the
 * version switch itself installs (`pnpmPackageNameToInstall`). A standalone
 * build keeps `@pnpm/exe`, except where that package cannot deliver a working
 * pnpm: from v12 the unscoped `pnpm` package is itself the native executable
 * and `@pnpm/exe` is no longer published alongside it, and v11+ ships no
 * darwin-x64 `@pnpm/exe` because a Node.js SEA build segfaults at startup on
 * Intel Macs (https://github.com/pnpm/pnpm/issues/11423). Naming `@pnpm/exe`
 * in either case would resolve to the newest release that has it and silently
 * strand the user there.
 */
function updatePkgName ({ latestVersion, proc }: UpdateMessageOptions): string {
  if (!detectIfCurrentPkgIsExecutable(proc)) return 'pnpm'
  const major = semver.major(latestVersion)
  if (major >= 12) return 'pnpm'
  if (major >= 11 && proc.platform === 'darwin' && proc.arch === 'x64') return 'pnpm'
  return '@pnpm/exe'
}
