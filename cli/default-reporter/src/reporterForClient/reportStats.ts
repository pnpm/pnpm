import { type StatsLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, take, reduce, map } from 'rxjs/operators'
import chalk from 'chalk'
import repeat from 'ramda/src/repeat'
import stringLength from 'string-length'
import { EOL } from '../constants'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants'
import { zoomOut } from './utils/zooming'

export function reportStats (
  log$: {
    stats: Rx.Observable<StatsLog>
  },
  opts: {
    cmd: string
    cwd: string
    isRecursive: boolean
    width: number
    hideProgressPrefix?: boolean
  }
): Array<Rx.Observable<Rx.Observable<{ msg: string }>>> {
  if (opts.hideProgressPrefix) {
    return [statsForCurrentPackage(log$.stats, {
      cmd: opts.cmd,
      width: opts.width,
    })]
  }
  const stats$ = opts.isRecursive
    ? log$.stats
    : log$.stats.pipe(filter((log) => log.prefix !== opts.cwd))

  const outputs = [
    statsForNotCurrentPackage(stats$, {
      cmd: opts.cmd,
      currentPrefix: opts.cwd,
      width: opts.width,
    }),
  ]

  if (!opts.isRecursive) {
    outputs.push(statsForCurrentPackage(log$.stats.pipe(
      filter((log) => log.prefix === opts.cwd)
    ), {
      cmd: opts.cmd,
      width: opts.width,
    }))
  }

  return outputs
}

function statsForCurrentPackage (
  stats$: Rx.Observable<StatsLog>,
  opts: {
    cmd: string
    width: number
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return stats$.pipe(
    take((opts.cmd === 'install' || opts.cmd === 'install-test' || opts.cmd === 'add' || opts.cmd === 'update' || opts.cmd === 'dlx') ? 2 : 1),
    reduce((acc, log) => {
      if (typeof log['added'] === 'number') {
        acc['added'] = log['added']
      } else if (typeof log['removed'] === 'number') {
        acc['removed'] = log['removed']
      }
      return acc
    }, {} as { added?: number, removed?: number }),
    map((stats) => {
      if (!stats['removed'] && !stats['added']) {
        if (opts.cmd === 'link') {
          return Rx.NEVER
        }
        return Rx.of({ msg: 'Already up to date' })
      }

      let msg = 'Packages:'
      if (stats['added']) {
        msg += ' ' + chalk.green(`+${stats['added'].toString()}`)
      }
      if (stats['removed']) {
        msg += ' ' + chalk.red(`-${stats['removed'].toString()}`)
      }
      msg += EOL + printPlusesAndMinuses(opts.width, (stats['added'] ?? 0), (stats['removed'] ?? 0))
      return Rx.of({ msg })
    })
  )
}

function statsForNotCurrentPackage (
  stats$: Rx.Observable<StatsLog>,
  opts: {
    cmd: string
    currentPrefix: string
    width: number
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const stats: Record<string, StatsLog> = {}
  type CookedStats =
    | { prefix: string, added?: number, removed?: number }
    | { seed: typeof stats, value: null, prefix?: never, added?: never, removed?: never }
  const cookedStats$ = (
    opts.cmd !== 'remove'
      ? stats$.pipe(
        map((log): CookedStats => {
          // As of pnpm v2.9.0, during `pnpm recursive link`, logging of removed stats happens twice
          //  1. during linking
          //  2. during installing
          // Hence, the stats are added before reported
          const { prefix } = log
          if (!stats[prefix]) {
            stats[prefix] = log
            return { seed: stats, value: null }
          } else if (typeof stats[prefix].added === 'number' && typeof log['added'] === 'number') {
            stats[prefix].added += log['added']
            return { seed: stats, value: null }
          } else if (typeof stats[prefix].removed === 'number' && typeof log['removed'] === 'number') {
            stats[prefix].removed += log['removed']
            return { seed: stats, value: null }
          } else {
            const value = { ...stats[prefix], ...log }
            delete stats[prefix]
            return value
          }
        }, {})
      )
      : stats$
  )
  return cookedStats$.pipe(
    filter((stats) => stats !== null && Boolean(stats['removed'] || stats['added'])), // eslint-disable-line @typescript-eslint/prefer-nullish-coalescing
    map((stats) => {
      const parts = [] as string[]

      if (stats['added']) {
        parts.push(padStep(chalk.green(`+${stats['added'].toString()}`), 4))
      }
      if (stats['removed']) {
        parts.push(padStep(chalk.red(`-${stats['removed'].toString()}`), 4))
      }

      let msg = zoomOut(opts.currentPrefix, stats.prefix!, parts.join(' '))
      const rest = Math.max(0, opts.width - 1 - stringLength(msg))
      // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
      msg += ' ' + printPlusesAndMinuses(rest, roundStats(stats['added'] || 0), roundStats(stats['removed'] || 0))
      return Rx.of({ msg })
    })
  )
}

function padStep (s: string, step: number): string {
  const sLength = stringLength(s)
  const placeholderLength = Math.ceil(sLength / step) * step
  if (sLength < placeholderLength) {
    return repeat(' ', placeholderLength - sLength).join('') + s
  }
  return s
}

function roundStats (stat: number): number {
  if (stat === 0) return 0
  return Math.max(1, Math.round(stat / 10))
}

function printPlusesAndMinuses (maxWidth: number, added: number, removed: number): string {
  if (maxWidth === 0) return ''
  const changes = added + removed
  let addedChars: number
  let removedChars: number
  if (changes > maxWidth) {
    if (!added) {
      addedChars = 0
      removedChars = maxWidth
    } else if (!removed) {
      addedChars = maxWidth
      removedChars = 0
    } else {
      const p = maxWidth / changes
      addedChars = Math.min(Math.max(Math.floor(added * p), 1), maxWidth - 1)
      removedChars = maxWidth - addedChars
    }
  } else {
    addedChars = added
    removedChars = removed
  }
  return `${repeat(ADDED_CHAR, addedChars).join('')}${repeat(REMOVED_CHAR, removedChars).join('')}`
}
