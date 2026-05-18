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
            msg: `${chalk.cyan('?')} Verifying lockfile${path_} against supply-chain policies (${entries})...`,
          }
        case 'done':
          return {
            msg: `${chalk.green('✓')} Lockfile${path_} passes supply-chain policies (${entries} in ${prettyMs(log.elapsedMs)})`,
          }
        case 'failed':
          // Brief one-liner so the transient `started` frame doesn't
          // stay on screen above the detailed PnpmError block that the
          // error reporter prints next.
          return {
            msg: `${chalk.red('✗')} Lockfile${path_} failed supply-chain policy check (${entries} in ${prettyMs(log.elapsedMs)})`,
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
//
// Uses `path.relative` rather than a strict `===` between
// `path.dirname(lockfilePath)` and `expectedDir`: relative path
// computation normalizes slash direction and trailing separators, so
// a workspaceDir like `C:/repo/` correctly matches a lockfilePath at
// `C:\repo\pnpm-lock.yaml` on Windows. The lockfile is considered
// "inside the expected dir" when the relative path is a bare file
// name (no separator) that doesn't escape upward.
function formatLockfilePath (
  lockfilePath: string | undefined,
  cwd: string,
  expectedDir: string
): string {
  if (lockfilePath == null) return ''
  const fromExpected = path.relative(expectedDir, lockfilePath)
  const isDirectChild = !fromExpected.includes(path.sep) && !fromExpected.startsWith('..')
  if (isDirectChild) return ''
  return ` at ${normalize(path.relative(cwd, lockfilePath))}`
}
