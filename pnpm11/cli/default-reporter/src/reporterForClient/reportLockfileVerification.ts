import path from 'node:path'

import type { LockfileVerificationLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import normalize from 'normalize-path'
import prettyMs from 'pretty-ms'
import * as Rx from 'rxjs'
import { concatMap } from 'rxjs/operators'

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
): Rx.Observable<Rx.Observable<{ msg: string, fixed?: boolean }>> {
  const expectedDir = opts.workspaceDir ?? opts.cwd
  // A single inner observable so the `done` message overwrites the
  // transient `started` message when the reporter redraws in place. In
  // appendOnly mode both lines are printed.
  return Rx.of(lockfileVerification$.pipe(
    // The cached verdict is a one-shot status: it confirms the policy gate
    // ran via a cached short-circuit and shouldn't be re-rendered on every
    // subsequent progress tick. Without this expansion a single cached
    // emission produces dozens of duplicate lines in any captured output
    // (CI logs, `tee`, `script`, terminal scrollback) because `mergeOutputs`
    // keeps non-fixed blocks in `acc.blocks` for the rest of the command
    // and re-includes them in every redraw.
    //
    // The fix uses the same fixed-then-clear pattern `reportProgress` uses
    // for the "done" handoff: emit the cached line as `fixed: true` so it
    // lands in `acc.fixedBlocks`, then immediately emit an empty non-fixed
    // frame whose `prevFixedBlockNo` (computed by `mergeOutputs` from this
    // inner observable's most recent fixed emission) deletes the cached
    // block on the next scan. The empty `msg` renders nothing; its only
    // job is to carry the fixed-block deletion. After this, subsequent
    // progress events from other reporter sources no longer re-include
    // the cached line in the combined frame.
    concatMap((log) => {
      const path_ = formatLockfilePath(log.lockfilePath, opts.cwd, expectedDir)
      if (log.status === 'cached') {
        // Two emissions in sequence: the cached verdict as `fixed: true`
        // (lands in `acc.fixedBlocks`), then an empty non-fixed emission
        // (carries `prevFixedBlockNo` = the cached fixed block's index, so
        // `mergeOutputs` deletes it on the next scan). The empty `msg` is
        // filtered out of the rendered output by `filter(Boolean)` and
        // adds no visible content; its only job is to delete the fixed
        // block so subsequent progress redraws no longer re-include the
        // cached line.
        return Rx.concat(
          Rx.of({
            msg: `${chalk.green('✓')} Lockfile${path_} passes supply-chain policies (${formatCachedVerdict(log.verifiedAt)})`,
            fixed: true,
          }),
          Rx.of({ msg: '', fixed: false })
        )
      }
      const entries = `${log.entries} ${log.entries === 1 ? 'entry' : 'entries'}`
      switch (log.status) {
        case 'started':
          return Rx.of({
            msg: `${chalk.cyan('?')} Verifying lockfile${path_} against supply-chain policies (${entries})...`,
          })
        case 'done':
          return Rx.of({
            msg: `${chalk.green('✓')} Lockfile${path_} passes supply-chain policies (${entries} in ${prettyMs(log.elapsedMs)})`,
          })
        case 'failed':
          // Brief one-liner so the transient `started` frame doesn't
          // stay on screen above the detailed PnpmError block that the
          // error reporter prints next.
          return Rx.of({
            msg: `${chalk.red('✗')} Lockfile${path_} failed supply-chain policy check (${entries} in ${prettyMs(log.elapsedMs)})`,
          })
      }
    })
  ))
}

// Relative "verified 2h ago" when the cached record carries a parseable
// timestamp; the timeless "previously verified" otherwise. The elapsed
// time is clamped at zero so a clock that moved backwards between the
// verification run and this install doesn't render a negative age.
function formatCachedVerdict (verifiedAt: string | undefined): string {
  if (verifiedAt == null) return 'previously verified'
  const elapsedMs = Date.now() - Date.parse(verifiedAt)
  if (Number.isNaN(elapsedMs)) return 'previously verified'
  return `verified ${prettyMs(Math.max(elapsedMs, 0), { compact: true })} ago`
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
