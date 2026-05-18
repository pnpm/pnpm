import path from 'node:path'

import type { LockfileVerificationLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import normalize from 'normalize-path'
import prettyMs from 'pretty-ms'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'

export interface ReportLockfileVerificationOptions {
  cwd: string
  /**
   * The workspace root, when one exists. Used as the "expected"
   * location for the lockfile — when the lockfile lives there, the
   * path is implied and we don't repeat it in the rendered message.
   * Falls back to `cwd` for single-project installs.
   */
  workspaceDir?: string
}

export function reportLockfileVerification (
  lockfileVerification$: Rx.Observable<LockfileVerificationLog>,
  opts: ReportLockfileVerificationOptions
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const expectedDir = opts.workspaceDir ?? opts.cwd
  // A single inner observable so the `done` message overwrites the
  // transient `started` message in ansi-diff mode. In appendOnly mode
  // both lines are printed.
  return Rx.of(lockfileVerification$.pipe(
    map((log) => {
      const path_ = formatLockfilePath(log.lockfilePath, opts.cwd, expectedDir)
      const entries = `${log.entries} ${log.entries === 1 ? 'entry' : 'entries'}`
      switch (log.status) {
        case 'started':
          return {
            msg: `${chalk.cyan('?')} Verifying lockfile${path_} (${entries})...`,
          }
        case 'done':
          return {
            msg: `${chalk.green('✓')} Lockfile${path_} verified (${entries} in ${prettyMs(log.elapsedMs)})`,
          }
      }
    })
  ))
}

// Returns a leading-space-prefixed `at <path>` suffix only when the
// lockfile sits outside the obvious project/workspace root — otherwise
// the path is implied and printing it would just add noise to every
// install. Empty string when the path is omitted or matches the
// expected location.
function formatLockfilePath (
  lockfilePath: string | undefined,
  cwd: string,
  expectedDir: string
): string {
  if (lockfilePath == null) return ''
  if (path.dirname(lockfilePath) === expectedDir) return ''
  return ` at ${normalize(path.relative(cwd, lockfilePath))}`
}
