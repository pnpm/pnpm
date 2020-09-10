import { StatsLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, take, reduce, map } from 'rxjs/operators'
import { EOL } from '../constants'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants'
import { zoomOut } from './utils/zooming'
import chalk = require('chalk')
import R = require('ramda')
import stringLength = require('string-length')

export default (
  log$: {
    stats: Rx.Observable<StatsLog>
  },
  opts: {
    cmd: string
    cwd: string
    isRecursive: boolean
    width: number
  }
) => {
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
    outputs.push(statsForCurrentPackage(log$.stats, {
      cmd: opts.cmd,
      currentPrefix: opts.cwd,
      width: opts.width,
    }))
  }

  return outputs
}

function statsForCurrentPackage (
  stats$: Rx.Observable<StatsLog>,
  opts: {
    cmd: string
    currentPrefix: string
    width: number
  }
) {
  return stats$.pipe(
    filter((log) => log.prefix === opts.currentPrefix),
    take((opts.cmd === 'install' || opts.cmd === 'install-test' || opts.cmd === 'add' || opts.cmd === 'update') ? 2 : 1),
    reduce((acc, log) => {
      if (typeof log['added'] === 'number') {
        acc['added'] = log['added']
      } else if (typeof log['removed'] === 'number') {
        acc['removed'] = log['removed']
      }
      return acc
    }, {}),
    map((stats) => {
      if (!stats['removed'] && !stats['added']) {
        if (opts.cmd === 'link') {
          return Rx.NEVER
        }
        return Rx.of({ msg: 'Already up-to-date' })
      }

      let msg = 'Packages:'
      if (stats['added']) {
        // eslint-disable-next-line @typescript-eslint/restrict-template-expressions
        msg += ' ' + chalk.green(`+${stats['added'].toString()}`)
      }
      if (stats['removed']) {
        // eslint-disable-next-line @typescript-eslint/restrict-template-expressions
        msg += ' ' + chalk.red(`-${stats['removed'].toString()}`)
      }
      msg += EOL + printPlusesAndMinuses(opts.width, (stats['added'] || 0), (stats['removed'] || 0))
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
) {
  const stats = {}
  const cookedStats$ = (
    opts.cmd !== 'remove'
      ? stats$.pipe(
        map((log) => {
          // As of pnpm v2.9.0, during `pnpm recursive link`, logging of removed stats happens twice
          //  1. during linking
          //  2. during installing
          // Hence, the stats are added before reported
          if (!stats[log.prefix]) {
            stats[log.prefix] = log
            return { seed: stats, value: null }
          } else if (typeof stats[log.prefix].added === 'number' && typeof log['added'] === 'number') {
            stats[log.prefix].added += log['added'] // eslint-disable-line
            return { seed: stats, value: null }
          } else if (typeof stats[log.prefix].removed === 'number' && typeof log['removed'] === 'number') {
            stats[log.prefix].removed += log['removed'] // eslint-disable-line
            return { seed: stats, value: null }
          } else {
            const value = { ...stats[log.prefix], ...log }
            delete stats[log.prefix]
            return value
          }
        }, {})
      )
      : stats$
  )
  return cookedStats$.pipe(
    filter((stats) => stats !== null && (stats['removed'] || stats['added'])),
    map((stats) => {
      const parts = [] as string[]

      if (stats['added']) {
        // eslint-disable-next-line @typescript-eslint/restrict-template-expressions
        parts.push(padStep(chalk.green(`+${stats['added'].toString()}`), 4))
      }
      if (stats['removed']) {
        // eslint-disable-next-line @typescript-eslint/restrict-template-expressions
        parts.push(padStep(chalk.red(`-${stats['removed'].toString()}`), 4))
      }

      let msg = zoomOut(opts.currentPrefix, stats['prefix'], parts.join(' '))
      const rest = Math.max(0, opts.width - 1 - stringLength(msg))
      msg += ' ' + printPlusesAndMinuses(rest, roundStats(stats['added'] || 0), roundStats(stats['removed'] || 0))
      return Rx.of({ msg })
    })
  )
}

function padStep (s: string, step: number) {
  const sLength = stringLength(s)
  const placeholderLength = Math.ceil(sLength / step) * step
  if (sLength < placeholderLength) {
    return R.repeat(' ', placeholderLength - sLength).join('') + s
  }
  return s
}

function roundStats (stat: number): number {
  if (stat === 0) return 0
  return Math.max(1, Math.round(stat / 10))
}

function printPlusesAndMinuses (maxWidth: number, added: number, removed: number) {
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
  return `${R.repeat(ADDED_CHAR, addedChars).join('')}${R.repeat(REMOVED_CHAR, removedChars).join('')}`
}
