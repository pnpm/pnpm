import type { StatsLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import { repeat } from 'ramda'
import * as Rx from 'rxjs'
import { filter, map, reduce, take } from 'rxjs/operators'
import stringLength from 'string-length'

import { EOL } from '../constants.js'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants.js'
import { zoomOut } from './utils/zooming.js'

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

// These commands log the stats of the current package twice: once for the
// added and once for the removed packages.
const COMMANDS_WITH_TWO_STATS_LOGS = new Set(['install', 'install-test', 'add', 'update', 'dlx'])

interface PackageStats {
  added?: number
  removed?: number
}

function statsForCurrentPackage (
  stats$: Rx.Observable<StatsLog>,
  opts: {
    cmd: string
    width: number
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return stats$.pipe(
    take(COMMANDS_WITH_TWO_STATS_LOGS.has(opts.cmd) ? 2 : 1),
    reduce(addStatsLog, {} as PackageStats),
    map((stats) => {
      if (!stats['removed'] && !stats['added']) {
        if (opts.cmd === 'link') {
          return Rx.NEVER
        }
        return Rx.of({ msg: 'Already up to date' })
      }
      return Rx.of({ msg: formatCurrentPackageStats(stats, opts.width) })
    })
  )
}

function addStatsLog (acc: PackageStats, log: StatsLog): PackageStats {
  if (typeof log['added'] === 'number') {
    acc['added'] = log['added']
  } else if (typeof log['removed'] === 'number') {
    acc['removed'] = log['removed']
  }
  return acc
}

function formatCurrentPackageStats (stats: PackageStats, width: number): string {
  let msg = 'Packages:'
  if (stats['added']) {
    msg += ' ' + chalk.green(`+${stats['added'].toString()}`)
  }
  if (stats['removed']) {
    msg += ' ' + chalk.red(`-${stats['removed'].toString()}`)
  }
  msg += EOL + printPlusesAndMinuses(width, (stats['added'] ?? 0), (stats['removed'] ?? 0))
  return msg
}

type StatsByPrefix = Record<string, StatsLog>

type CookedStats =
  | { prefix: string, added?: number, removed?: number }
  | { seed: StatsByPrefix, value: null, prefix?: never, added?: never, removed?: never }

function statsForNotCurrentPackage (
  stats$: Rx.Observable<StatsLog>,
  opts: {
    cmd: string
    currentPrefix: string
    width: number
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const stats: StatsByPrefix = {}
  const cookedStats$ = (
    opts.cmd !== 'remove'
      ? stats$.pipe(
        map((log): CookedStats => mergeStatsLog(stats, log), {})
      )
      : stats$
  )
  return cookedStats$.pipe(
    filter((stats) => stats !== null && Boolean(stats['removed'] || stats['added'])),
    map((stats) => Rx.of({ msg: formatNotCurrentPackageStats(stats, opts) }))
  )
}

function mergeStatsLog (stats: StatsByPrefix, log: StatsLog): CookedStats {
  // As of pnpm v2.9.0, during `pnpm recursive link`, logging of removed stats happens twice
  //  1. during linking
  //  2. during installing
  // Hence, the stats are added before reported
  const { prefix } = log
  if (!stats[prefix]) {
    stats[prefix] = log
    return { seed: stats, value: null }
  }
  if (typeof stats[prefix].added === 'number' && typeof log['added'] === 'number') {
    stats[prefix].added += log['added']
    return { seed: stats, value: null }
  }
  if (typeof stats[prefix].removed === 'number' && typeof log['removed'] === 'number') {
    stats[prefix].removed += log['removed']
    return { seed: stats, value: null }
  }
  const value = { ...stats[prefix], ...log }
  delete stats[prefix]
  return value
}

function formatNotCurrentPackageStats (
  stats: CookedStats,
  opts: { currentPrefix: string, width: number }
): string {
  const parts = [] as string[]

  if (stats['added']) {
    parts.push(padStep(chalk.green(`+${stats['added'].toString()}`), 4))
  }
  if (stats['removed']) {
    parts.push(padStep(chalk.red(`-${stats['removed'].toString()}`), 4))
  }

  let msg = zoomOut(opts.currentPrefix, stats.prefix!, parts.join(' '))
  const rest = Math.max(0, opts.width - 1 - stringLength(msg))
  msg += ' ' + printPlusesAndMinuses(rest, roundStats(stats['added'] || 0), roundStats(stats['removed'] || 0))
  return msg
}

function padStep (text: string, step: number): string {
  const textLength = stringLength(text)
  const placeholderLength = Math.ceil(textLength / step) * step
  if (textLength < placeholderLength) {
    return repeat(' ', placeholderLength - textLength).join('') + text
  }
  return text
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
      const charsPerChange = maxWidth / changes
      addedChars = Math.min(Math.max(Math.floor(added * charsPerChange), 1), maxWidth - 1)
      removedChars = maxWidth - addedChars
    }
  } else {
    addedChars = added
    removedChars = removed
  }
  return `${repeat(ADDED_CHAR, addedChars).join('')}${repeat(REMOVED_CHAR, removedChars).join('')}`
}
