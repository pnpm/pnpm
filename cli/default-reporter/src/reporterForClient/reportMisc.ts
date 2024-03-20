import os from 'node:os'

import * as Rx from 'rxjs'
import { filter, map } from 'rxjs/operators'

import type { Config, Log, PeerDependencyRules, RegistryLog } from '@pnpm/types'

import { autozoom } from './utils/zooming'
import { reportError } from '../reportError'
import { formatWarn } from './utils/formatWarn'
import { LogLevel } from '@pnpm/logger'

// eslint-disable:object-literal-sort-keys
export const LOG_LEVEL_NUMBER: Record<LogLevel, number> = {
  error: 0,
  warn: 1,
  info: 2,
  debug: 3,
}
// eslint-enable:object-literal-sort-keys

const MAX_SHOWN_WARNINGS = 5

export function reportMisc(
  log$: {
    registry: Rx.Observable<RegistryLog>
    other: Rx.Observable<Log>
  },
  opts: {
    appendOnly: boolean
    cwd: string
    logLevel?: LogLevel
    config?: Config
    zoomOutCurrent: boolean
    peerDependencyRules?: PeerDependencyRules
  }
) {
  const maxLogLevel =
    LOG_LEVEL_NUMBER[opts.logLevel ?? 'info'] ?? LOG_LEVEL_NUMBER.info
  const reportWarning = makeWarningReporter(opts)
  return Rx.merge(log$.registry, log$.other).pipe(
    filter(
      (obj) =>
        LOG_LEVEL_NUMBER[obj.level] <= maxLogLevel &&
        (obj.level !== 'info' || !obj.prefix || obj.prefix === opts.cwd)
    ),
    map((obj) => {
      switch (obj.level) {
        case 'warn': {
          return reportWarning(obj)
        }
        case 'error': {
          const errorOutput = reportError(
            obj,
            opts.config,
            opts.peerDependencyRules
          )
          if (!errorOutput) return Rx.NEVER
          if ('prefix' in obj && obj.prefix !== opts.cwd) {
            return Rx.of({
              msg: `${obj.prefix as string}:` + os.EOL + errorOutput,
            })
          }
          return Rx.of({ msg: errorOutput })
        }
        default: {
          return Rx.of({
            msg:
              'message' in obj && typeof obj.message === 'string'
                ? obj.message
                : '',
          })
        }
      }
    })
  )
}

// Sometimes, when installing new dependencies that rely on many peer dependencies,
// or when running installation on a huge monorepo, there will be hundreds or thousands of warnings.
// Printing many messages to the terminal is expensive and reduces speed,
// so pnpm will only print a few warnings and report the total number of the unprinted warnings.
function makeWarningReporter(opts: {
  appendOnly: boolean
  cwd: string
  zoomOutCurrent: boolean
}) {
  let warningsCounter = 0
  let collapsedWarnings: Rx.Subject<{ msg: string }>
  return (obj: { prefix: string; message: string }) => {
    warningsCounter++
    if (opts.appendOnly || warningsCounter <= MAX_SHOWN_WARNINGS) {
      return Rx.of({
        msg: autozoom(opts.cwd, obj.prefix, formatWarn(obj.message), opts),
      })
    }
    const warningMsg = formatWarn(
      `${warningsCounter - MAX_SHOWN_WARNINGS} other warnings`
    )
    if (!collapsedWarnings) {
      collapsedWarnings = new Rx.Subject()
      // For some reason, without using setTimeout, the warning summary is printed above the rest of the warnings
      // Even though the summary event happens last. Probably a bug in "most".
      setTimeout(() => {
        collapsedWarnings.next({ msg: warningMsg })
      }, 0)
      return Rx.from(collapsedWarnings)
    }
    setTimeout(() => {
      collapsedWarnings.next({ msg: warningMsg })
    }, 0)
    return Rx.NEVER
  }
}
