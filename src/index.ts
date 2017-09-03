import logUpdate = require('log-update')
import chalk = require('chalk')
import {
  ProgressLog,
  LifecycleLog,
  Log,
  InstallCheckLog,
  DeprecationLog,
  RegistryLog,
} from 'pnpm-logger'
import reportError from './reportError'
import os = require('os')
import xs, {Stream} from 'xstream'
import flattenConcurrently from 'xstream/extra/flattenConcurrently'
import dropRepeats from 'xstream/extra/dropRepeats'
import fromEvent from 'xstream/extra/fromEvent'
import R = require('ramda')
import {EventEmitter} from 'events'

const EOL = os.EOL

const addedSign = chalk.green('+')
const removedSign = chalk.red('-')

type PackageDiff = {
  name: string,
  version?: string,
  added: boolean,
  deprecated?: boolean,
}

const propertyByDependencyType = {
  prod: 'dependencies',
  dev: 'devDependencies',
  optional: 'optionalDependencies',
}

export default function (streamParser: Object) {
  toOutput$(streamParser)
    .subscribe({
      next: logUpdate,
      error: err => logUpdate(err.message),
      complete () {},
    })
}

export function toOutput$ (streamParser: Object) {
  const obs = fromEvent(streamParser as EventEmitter, 'data')
  const log$ = xs.fromObservable<Log>(obs)

  const progressLog$ = log$
    .filter(log => log.name === 'pnpm:progress') as Stream<ProgressLog>

  const resolvingContentLog$ = progressLog$
    .filter(log => log.status === 'resolving_content')
    .fold(R.inc, 0)
    .drop(1)

  const fedtchedLog$ = progressLog$
    .filter(log => log.status === 'fetched')
    .fold(R.inc, 0)

  const foundInStoreLog$ = progressLog$
    .filter(log => log.status === 'found_in_store')
    .fold(R.inc, 0)

  const resolutionDone$ = log$
    .filter(log => log.name === 'pnpm:stage' && log.message === 'resolution_done')
    .mapTo(true)
    .take(1)
    .startWith(false)

  const progressSummaryOutput$ = xs.of(
    xs.combine(
      resolvingContentLog$,
      fedtchedLog$,
      foundInStoreLog$,
      resolutionDone$
    )
    .map(
      R.apply((resolving, fetched, foundInStore: number, resolutionDone) => {
        const msg = `Resolving: total ${resolving}, reused ${foundInStore}, downloaded ${fetched}`
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
      })
    )
  )

  const deprecationLog$ = log$
    .filter(log => log.name === 'pnpm:deprecation') as Stream<DeprecationLog>

  const deprecationSet$ = deprecationLog$
    .fold((acc, log) => {
      acc.add(log.pkgId)
      return acc
    }, new Set())

  const rootLog$ = log$.filter(log => log.name === 'pnpm:root')

  const pkgsDiff$ = xs.combine(
    rootLog$,
    deprecationSet$
  )
  .fold((pkgsDiff, args) => {
    const rootLog = args[0]
    const deprecationSet = args[1] as Set<string>
    if (rootLog['added']) {
      pkgsDiff[rootLog['added'].dependencyType].push({
        name: rootLog['added'].name,
        version: rootLog['added'].version,
        deprecated: deprecationSet.has(rootLog['added'].id),
        added: true,
      })
      return pkgsDiff
    }
    if (rootLog['removed']) {
      pkgsDiff[rootLog['removed'].dependencyType].push({
        name: rootLog['removed'].name,
        version: rootLog['removed'].version,
        added: false,
      })
      return pkgsDiff
    }
    return pkgsDiff
  }, {
    prod: [],
    dev: [],
    optional: [],
  } as {
    prod: PackageDiff[],
    dev: PackageDiff[],
    optional: PackageDiff[],
  })

  const summaryLog$ = log$
    .filter(log => log.name === 'pnpm:summary')
    .take(1)

  const summaryOutput$ = xs.combine(
    pkgsDiff$,
    summaryLog$
  )
  .map(R.apply(pkgsDiff => {
    let msg = ''
    for (const depType of ['prod', 'optional', 'dev']) {
      if (pkgsDiff[depType].length) {
        msg += EOL
        msg += chalk.blue(`${propertyByDependencyType[depType]}:`)
        msg += EOL
        msg += printDiffs(pkgsDiff[depType])
        msg += EOL
      }
    }
    return {msg}
  }))
  .take(1)
  .map(xs.of)

  const deprecationOutput$ = deprecationLog$
    // print warnings only about deprecated packages from the root
    .filter(log => log.depth === 0)
    .map(log => {
      return {
        msg: formatWarn(`${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}: ${log.deprecated}`)
      }
    })
    .map(xs.of)

  const lifecycleOutput$ = log$
    .filter(log => log.name === 'pnpm:lifecycle')
    .map(formatLifecycle)
    .map(msg => ({msg}))
    .map(xs.of)

  const installCheckOutput$ = log$
    .filter(log => log.name === 'pnpm:install-check')
    .map(formatInstallCheck)
    .filter(Boolean)
    .map(msg => ({msg}))
    .map(xs.of) as Stream<Stream<{msg: string}>>

  const registryOutput$ = log$
    .filter(log => log.name === 'pnpm:registry' && log.level === 'warn')
    .map((log: RegistryLog) => ({msg: formatWarn(log.message)}))
    .map(xs.of)

  const miscOutput$ = log$
    .filter(log => log.name === 'pnpm')
    .map(obj => {
      if (obj.level === 'debug') return
      if (obj.level === 'warn') {
        return formatWarn(obj['message'])
      }
      if (obj.level === 'error') {
        return reportError(obj)
      }
      return obj['message']
    })
    .map(msg => ({msg}))
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
    )
    .map((log: Stream<{msg: string, fixed: boolean}>) => {
      let currentBlockNo = -1
      let currentFixedBlockNo = -1
      let calculated = false
      let fixedCalculated = false
      return log
        .map(msg => {
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
            prevFixedBlockNo: currentFixedBlockNo,
            blockNo: currentBlockNo,
            fixed: false,
            msg: typeof msg === 'string' ? msg : msg.msg,
          }
        })
    })
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
  .map(sections => (sections.blocks.concat(sections.fixedBlocks)).filter(Boolean).join(EOL))
  .filter(msg => {
    if (started) {
      return true
    }
    if (msg === '') return false
    started = true
    return true
  })
  .compose(dropRepeats())
}

function printDiffs (pkgsDiff: PackageDiff[]) {
  // Sorts by alphabet then by removed/added
  // + ava 0.10.0
  // - chalk 1.0.0
  // + chalk 2.0.0
  pkgsDiff.sort((a, b) => (a.name.localeCompare(b.name) * 10 + (Number(!b.added) - Number(!a.added))))
  const msg = pkgsDiff.map(pkg => {
    let result = pkg.added ? addedSign : removedSign
    result += ` ${pkg.name}`
    if (pkg.version) {
      result += ` ${chalk.grey(pkg.version)}`
    }
    if (pkg.deprecated) {
      result += ` ${chalk.red('deprecated')}`
    }
    return result
  }).join(EOL)
  return msg
}

function formatLifecycle (logObj: LifecycleLog) {
  if (logObj.level === 'error') {
    return `${chalk.blue(logObj.pkgId)}! ${chalk.gray(logObj.line)}`
  }
  return `${chalk.blue(logObj.pkgId)}  ${chalk.gray(logObj.line)}`
}

function formatInstallCheck (logObj: InstallCheckLog) {
  switch (logObj.code) {
    case 'EBADPLATFORM':
      return formatWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`)
    case 'ENOTSUP':
      return logObj.toString()
    default:
      return
  }
}

function formatWarn (message: string) {
  return `${chalk.yellow('WARN')} ${message}`
}
