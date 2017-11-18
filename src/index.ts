import chalk from 'chalk'
import {EventEmitter} from 'events'
import logUpdate = require('log-update')
import os = require('os')
import prettyBytes = require('pretty-bytes')
import R = require('ramda')
import semver = require('semver')
import {
  DeprecationLog,
  InstallCheckLog,
  LifecycleLog,
  Log,
  ProgressLog,
  RegistryLog,
} from 'supi'
import xs, {Stream} from 'xstream'
import dropRepeats from 'xstream/extra/dropRepeats'
import flattenConcurrently from 'xstream/extra/flattenConcurrently'
import fromEvent from 'xstream/extra/fromEvent'
import getPkgsDiff, {
  PackageDiff,
  propertyByDependencyType,
} from './pkgsDiff'
import reportError from './reportError'

const EOL = os.EOL
const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

const addedSign = chalk.green('+')
const removedSign = chalk.red('-')
const linkSign = chalk.magentaBright('#')
const hlValue = chalk.blue
const hlPkgId = chalk['whiteBright']

export default function(streamParser: object) {
  toOutput$(streamParser)
    .subscribe({
      complete() {}, // tslint:disable-line:no-empty
      error: (err) => logUpdate(err.message),
      next: logUpdate,
    })
}

export function toOutput$(streamParser: object): Stream<string> {
  const obs = fromEvent(streamParser as EventEmitter, 'data')
  const log$ = xs.fromObservable<Log>(obs)

  const progressLog$ = log$
    .filter((log) => log.name === 'pnpm:progress') as Stream<ProgressLog>

  const resolutionDone$ = log$
    .filter((log) => log.name === 'pnpm:stage' && log.message === 'resolution_done')
    .mapTo(true)
    .take(1)
    .startWith(false)

  const resolvingContentLog$ = progressLog$
    .filter((log) => log.status === 'resolving_content')
    .fold(R.inc, 0)
    .drop(1)
    .endWhen(resolutionDone$.last())

  const fedtchedLog$ = progressLog$
    .filter((log) => log.status === 'fetched')
    .fold(R.inc, 0)

  const foundInStoreLog$ = progressLog$
    .filter((log) => log.status === 'found_in_store')
    .fold(R.inc, 0)

  const alreadyUpToDate$ = xs.of(
    resolvingContentLog$
      .take(1)
      .mapTo(false)
      .startWith(true)
      .last()
      .filter(R.equals(true))
      .mapTo({
        fixed: false,
        msg: 'Already up-to-date',
      }),
  )

  const progressSummaryOutput$ = xs.of(
    xs.combine(
      resolvingContentLog$,
      fedtchedLog$,
      foundInStoreLog$,
      resolutionDone$,
    )
    .map(
      R.apply((resolving, fetched, foundInStore: number, resolutionDone) => {
        const msg = `Resolving: total ${hlValue(resolving.toString())}, reused ${hlValue(foundInStore.toString())}, downloaded ${hlValue(fetched.toString())}`
        if (resolving === foundInStore + fetched && resolutionDone) {
          return {
            fixed: false,
            msg: `${msg}, done`,
          }
        }
        return {
          fixed: true,
          msg,
        }
      }),
    ),
  )

  const tarballsProgressOutput$ = progressLog$
    .filter((log) => log.status === 'fetching_started' &&
      typeof log.size === 'number' && log.size >= BIG_TARBALL_SIZE)
    .map((startedLog) => {
      const size = prettyBytes(startedLog['size'])
      return progressLog$
        .filter((log) => log.status === 'fetching_progress' && log.pkgId === startedLog['pkgId'])
        .map((log) => log['downloaded'])
        .startWith(0)
        .map((downloadedRaw) => {
          const done = startedLog['size'] === downloadedRaw
          const downloaded = prettyBytes(downloadedRaw)
          return {
            fixed: !done,
            msg: `Downloading ${hlPkgId(startedLog['pkgId'])}: ${hlValue(downloaded)}/${hlValue(size)}${done ? ', done' : ''}`,
          }
        })
    })

  const deprecationLog$ = log$
    .filter((log) => log.name === 'pnpm:deprecation') as Stream<DeprecationLog>

  const pkgsDiff$ = getPkgsDiff(log$, deprecationLog$)

  const summaryLog$ = log$
    .filter((log) => log.name === 'pnpm:summary')
    .take(1)

  const summaryOutput$ = xs.combine(
    pkgsDiff$,
    summaryLog$,
  )
  .map(R.apply((pkgsDiff) => {
    let msg = ''
    for (const depType of ['prod', 'optional', 'dev']) {
      const diffs = R.values(pkgsDiff[depType])
      if (diffs.length) {
        msg += EOL
        msg += chalk.blue(`${propertyByDependencyType[depType]}:`)
        msg += EOL
        msg += printDiffs(diffs)
        msg += EOL
      }
    }
    return {msg}
  }))
  .take(1)
  .map(xs.of)

  const deprecationOutput$ = deprecationLog$
    // print warnings only about deprecated packages from the root
    .filter((log) => log.depth === 0)
    .map((log) => {
      return {
        msg: formatWarn(`${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}: ${log.deprecated}`),
      }
    })
    .map(xs.of)

  const lifecycleMessages: {[pkgId: string]: string} = {}
  const lifecycleOutput$ = xs.of(
    log$
      .filter((log) => log.name === 'pnpm:lifecycle')
      .map((log: LifecycleLog) => {
        const key = `${log.script}:${log.pkgId}`
        lifecycleMessages[key] = formatLifecycle(log)
        return R.values(lifecycleMessages).join(EOL)
      })
      .map((msg) => ({msg})),
  )

  const installCheckOutput$ = log$
    .filter((log) => log.name === 'pnpm:install-check')
    .map(formatInstallCheck)
    .filter(Boolean)
    .map((msg) => ({msg}))
    .map(xs.of) as Stream<Stream<{msg: string}>>

  const registryOutput$ = log$
    .filter((log) => log.name === 'pnpm:registry' && log.level === 'warn')
    .map((log: RegistryLog) => ({msg: formatWarn(log.message)}))
    .map(xs.of)

  const miscOutput$ = log$
    .filter((log) => log.name as string === 'pnpm' || log.name as string === 'pnpm:link')
    .map((obj) => {
      if (obj.level === 'debug') return
      if (obj.level === 'warn') {
        return formatWarn(obj['message'])
      }
      if (obj.level === 'error') {
        return reportError(obj)
      }
      return obj['message']
    })
    .map((msg) => ({msg}))
    .map(xs.of)

  let blockNo = 0
  let fixedBlockNo = 0
  let started = false
  return flattenConcurrently(
    xs.merge(
      summaryOutput$,
      progressSummaryOutput$,
      registryOutput$,
      installCheckOutput$,
      lifecycleOutput$,
      deprecationOutput$,
      miscOutput$,
      tarballsProgressOutput$,
      alreadyUpToDate$,
    )
    .map((log: Stream<{msg: string, fixed: boolean}>) => {
      let currentBlockNo = -1
      let currentFixedBlockNo = -1
      let calculated = false
      let fixedCalculated = false
      return log
        .map((msg) => {
          if (msg['fixed']) {
            if (!fixedCalculated) {
              fixedCalculated = true
              currentFixedBlockNo = fixedBlockNo++
            }
            return {
              blockNo: currentFixedBlockNo,
              fixed: true,
              msg: msg.msg,
            }
          }
          if (!calculated) {
            calculated = true
            currentBlockNo = blockNo++
          }
          return {
            blockNo: currentBlockNo,
            fixed: false,
            msg: typeof msg === 'string' ? msg : msg.msg,
            prevFixedBlockNo: currentFixedBlockNo,
          }
        })
    }),
  )
  .fold((acc, log) => {
    if (log.fixed === true) {
      acc.fixedBlocks[log.blockNo] = log.msg
    } else {
      delete acc.fixedBlocks[log['prevFixedBlockNo']]
      acc.blocks[log.blockNo] = log.msg
    }
    return acc
  }, {fixedBlocks: [], blocks: []} as {fixedBlocks: string[], blocks: string[]})
  .map((sections) => {
    const fixedBlocks = sections.fixedBlocks.filter(Boolean)
    const nonFixedPart = sections.blocks.filter(Boolean).join(EOL)
    if (!fixedBlocks.length) {
      return nonFixedPart
    }
    const fixedPart = fixedBlocks.join(EOL)
    if (!nonFixedPart) {
      return fixedPart
    }
    return chalk.dim(nonFixedPart) + EOL + fixedPart
  })
  .filter((msg) => {
    if (started) {
      return true
    }
    if (msg === '') return false
    started = true
    return true
  })
  .compose(dropRepeats())
}

function printDiffs(pkgsDiff: PackageDiff[]) {
  // Sorts by alphabet then by removed/added
  // + ava 0.10.0
  // - chalk 1.0.0
  // + chalk 2.0.0
  pkgsDiff.sort((a, b) => (a.name.localeCompare(b.name) * 10 + (Number(!b.added) - Number(!a.added))))
  const msg = pkgsDiff.map((pkg) => {
    let result = pkg.added
      ? addedSign
      : pkg.linked
        ? linkSign
        : removedSign
    if (!pkg.realName || pkg.name === pkg.realName) {
      result += ` ${pkg.name}`
    } else {
      result += ` ${pkg.name} <- ${pkg.realName}`
    }
    if (pkg.version) {
      result += ` ${chalk.grey(pkg.version)}`
      if (pkg.latest && semver.lt(pkg.version, pkg.latest)) {
        result += ` ${chalk.grey(`(${pkg.latest} is available)`)}`
      }
    }
    if (pkg.deprecated) {
      result += ` ${chalk.red('deprecated')}`
    }
    if (pkg.linked) {
      result += ` ${chalk.magentaBright('linked from')} ${chalk.grey(pkg.from || '???')}`
    }
    return result
  }).join(EOL)
  return msg
}

function formatLifecycle(logObj: LifecycleLog) {
  const prefix = `Running ${hlValue(logObj.script)} for ${hlPkgId(logObj.pkgId)}`
  if (logObj['exitCode'] === 0) {
    return `${prefix}, done`
  }
  const line = formatLine(logObj)
  if (logObj.level === 'error') {
    return `${prefix}! ${line}`
  }
  return `${prefix}: ${line}`
}

function formatLine(logObj: LifecycleLog) {
  if (typeof logObj['exitCode'] === 'number') return chalk.red(`Exited with ${logObj['exitCode']}`)

  const color = logObj.level === 'error' ? chalk.red : chalk.gray
  return color(logObj['line'])
}

function formatInstallCheck(logObj: InstallCheckLog) {
  switch (logObj.code) {
    case 'EBADPLATFORM':
      return formatWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`)
    case 'ENOTSUP':
      return logObj.toString()
    default:
      return
  }
}

function formatWarn(message: string) {
  // The \u2009 is the "thin space" unicode character
  // It is used instead of ' ' because chalk (as of version 2.1.0)
  // trims whitespace at the beginning
  return `${chalk.bgYellow.black('\u2009WARN\u2009')} ${message}`
}
