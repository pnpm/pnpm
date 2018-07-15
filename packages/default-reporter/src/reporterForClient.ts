import chalk from 'chalk'
import most = require('most')
import normalize = require('normalize-path')
import os = require('os')
import path = require('path')
import prettyBytes = require('pretty-bytes')
import R = require('ramda')
import rightPad = require('right-pad')
import semver = require('semver')
import stringLength = require('string-length')
import padStart = require('string.prototype.padstart')
import stripAnsi = require('strip-ansi')
import {
  InstallCheckLog,
  LifecycleLog,
  RegistryLog,
} from 'supi'
import * as supi from 'supi'
import PushStream = require('zen-push')
import {EOL} from './constants'
import getPkgsDiff, {
  PackageDiff,
  propertyByDependencyType,
} from './pkgsDiff'
import reportError from './reportError'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

const ADDED_CHAR = chalk.green('+')
const REMOVED_CHAR = chalk.red('-')
const PREFIX_MAX_LENGTH = 40

const hlValue = chalk.cyanBright
const hlPkgId = chalk['whiteBright']

export default function (
  log$: {
    progress: most.Stream<supi.ProgressLog>,
    stage: most.Stream<supi.StageLog>,
    deprecation: most.Stream<supi.DeprecationLog>,
    summary: most.Stream<supi.SummaryLog>,
    lifecycle: most.Stream<supi.LifecycleLog>,
    stats: most.Stream<supi.StatsLog>,
    installCheck: most.Stream<supi.InstallCheckLog>,
    registry: most.Stream<supi.RegistryLog>,
    root: most.Stream<supi.RootLog>,
    packageJson: most.Stream<supi.PackageJsonLog>,
    link: most.Stream<supi.Log>,
    other: most.Stream<supi.Log>,
    cli: most.Stream<supi.Log>,
    hook: most.Stream<supi.Log>,
    skippedOptionalDependency: most.Stream<supi.SkippedOptionalDependencyLog>,
  },
  opts: {
    isRecursive: boolean,
    cmd: string,
    subCmd?: string,
    width?: number,
    appendOnly?: boolean,
    throttleProgress?: number,
    cwd: string,
  },
): Array<most.Stream<most.Stream<{msg: string}>>> {
  const width = opts.width || process.stdout.columns || 80
  const outputs: Array<most.Stream<most.Stream<{msg: string}>>> = []
  const cwd = opts.cwd || process.cwd()

  const resolutionDone$ = opts.isRecursive
    ? most.never()
    : log$.stage
      .filter((log) => log.message === 'resolution_done')

  const resolvingContentLog$ = log$.progress
    .filter((log) => log.status === 'resolving_content')
    .scan(R.inc, 0)
    .skip(1)
    .until(resolutionDone$)

  const fedtchedLog$ = log$.progress
    .filter((log) => log.status === 'fetched')
    .scan(R.inc, 0)

  const foundInStoreLog$ = log$.progress
    .filter((log) => log.status === 'found_in_store')
    .scan(R.inc, 0)

  function createStatusMessage (resolving: number, fetched: number, foundInStore: number, importingDone: boolean) {
    const msg = `Resolving: total ${hlValue(resolving.toString())}, reused ${hlValue(foundInStore.toString())}, downloaded ${hlValue(fetched.toString())}`
    if (importingDone) {
      return {
        done: true,
        fixed: false,
        msg: `${msg}, done`,
      }
    }
    return {
      fixed: true,
      msg,
    }
  }

  const importingDone$ = opts.isRecursive || opts.cmd === 'link'
    ? most.of(false)
    : log$.stage.filter((log) => log.message === 'importing_done')
      .constant(true)
      .take(1)
      .startWith(false)
      .multicast()

  if (typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0) {
    const resolutionStarted$ = log$.stage
      .filter((log) => log.message === 'resolution_started' || log.message === 'importing_started').take(1)
    const commandDone$ = log$.cli.filter((log) => log['message'] === 'command_done')

    // Reporting is done every `throttleProgress` milliseconds
    // and once all packages are fetched.
    const sampler = opts.isRecursive
      ? most.merge(most.periodic(opts.throttleProgress).until(commandDone$), commandDone$)
      : most.merge(
        most.periodic(opts.throttleProgress).since(resolutionStarted$).until(most.merge<{}>(importingDone$.skip(1), commandDone$)),
        importingDone$,
      )
    const progress = most.sample(
      createStatusMessage,
      sampler,
      resolvingContentLog$,
      fedtchedLog$,
      foundInStoreLog$,
      importingDone$,
    )
    // Avoid logs after all resolved packages were downloaded.
    // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
    .skipAfter((msg) => msg.done === true)

    outputs.push(most.of(progress))
  } else {
    const progress = most.combine(
      createStatusMessage,
      resolvingContentLog$,
      fedtchedLog$,
      foundInStoreLog$,
      opts.isRecursive ? most.of(false) : importingDone$,
    )
    outputs.push(most.of(progress))
  }

  // When the reporter is not append-only, the length of output is limited
  // in order to reduce flickering
  const formatLifecycle = formatLifecycleHideOverflow.bind(null, opts.appendOnly ? Infinity : width)
  if (!opts.appendOnly) {
    const tarballsProgressOutput$ = log$.progress
      .filter((log) => log.status === 'fetching_started' &&
        typeof log.size === 'number' && log.size >= BIG_TARBALL_SIZE &&
        // When retrying the download, keep the existing progress line.
        // Fixing issue: https://github.com/pnpm/pnpm/issues/1013
        log.attempt === 1)
      .map((startedLog) => {
        const size = prettyBytes(startedLog['size'])
        return log$.progress
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

    outputs.push(tarballsProgressOutput$)

    const lifecycleMessages: {
      [depPath: string]: {
        output: string[],
        script: string,
      },
    } = {}
    const lifecycleStreamByDepPath: {
      [depPath: string]: {
        observable: most.Observable<{msg: string}>,
        complete (): void,
        next (obj: object): void,
      },
    } = {}
    const lifecyclePushStream = new PushStream()
    outputs.push(most.from(lifecyclePushStream.observable))

    log$.lifecycle
      .forEach((log: LifecycleLog) => {
        const key = `${log.stage}:${log.depPath}`
        lifecycleMessages[key] = lifecycleMessages[key] || {output: []}
        if (log['script']) {
          lifecycleMessages[key].script = formatLifecycle(cwd, log)
        } else {
          if (!lifecycleMessages[key].output.length || log['exitCode'] !== 0) {
            lifecycleMessages[key].output.push(formatLifecycle(cwd, log))
          }
          if (lifecycleMessages[key].output.length > 3) {
            lifecycleMessages[key].output.shift()
          }
        }
        if (!lifecycleStreamByDepPath[key]) {
          lifecycleStreamByDepPath[key] = new PushStream()
          lifecyclePushStream.next(most.from(lifecycleStreamByDepPath[key].observable))
        }
        lifecycleStreamByDepPath[key].next({
          msg: EOL + [lifecycleMessages[key].script].concat(lifecycleMessages[key].output).join(EOL),
        })
        if (typeof log['exitCode'] === 'number') {
          lifecycleStreamByDepPath[key].complete()
        }
      })
  } else {
    const lifecycleMessages: {[pkgId: string]: string} = {}
    const lifecycleOutput$ = most.of(
      log$.lifecycle
        .map((log: LifecycleLog) => ({ msg: formatLifecycle(cwd, log) })),
    )

    outputs.push(lifecycleOutput$)
  }

  if (!opts.isRecursive) {
    const pkgsDiff$ = getPkgsDiff(log$, {prefix: opts.cwd})

    const summaryLog$ = log$.summary
      .take(1)

    const summaryOutput$ = most.combine(
      (pkgsDiff) => {
        let msg = ''
        for (const depType of ['prod', 'optional', 'dev', 'nodeModulesOnly']) {
          const diffs = R.values(pkgsDiff[depType])
          if (diffs.length) {
            msg += EOL
            msg += chalk.cyanBright(`${propertyByDependencyType[depType]}:`)
            msg += EOL
            msg += printDiffs(diffs, {prefix: opts.cwd})
            msg += EOL
          }
        }
        return {msg}
      },
      pkgsDiff$,
      summaryLog$,
    )
    .take(1)
    .map(most.of)

    outputs.push(summaryOutput$)
  }

  const deprecationOutput$ = log$.deprecation
    // print warnings only about deprecated packages from the root
    .filter((log) => log.depth === 0)
    .map((log) => {
      if (log.prefix === opts.cwd) {
        return {
          msg: formatWarn(`${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}: ${log.deprecated}`),
        }
      }
      return {
        msg: `${rightPad(formatPrefix(opts.cwd, log.prefix), PREFIX_MAX_LENGTH)} | ` +
          formatWarn(`${chalk.red('deprecated')} ${log.pkgName}@${log.pkgVersion}`),
      }
    })
    .map(most.of)

  outputs.push(deprecationOutput$)

  outputs.push(miscOutput(most.merge(log$.link, log$.registry, log$.other), {cwd: opts.cwd}))

  const stats$ = opts.isRecursive
    ? log$.stats
    : log$.stats.filter((log) => log.prefix !== cwd)
  outputs.push(statsForNotCurrentPackage(stats$, {
    currentPrefix: opts.cwd,
    subCmd: opts.subCmd,
    width,
  }))

  if (!opts.isRecursive) {
    outputs.push(statsForCurrentPackage(log$.stats, {
      cmd: opts.cmd,
      currentPrefix: opts.cwd,
      width,
    }))
  }

  const installCheckOutput$ = log$.installCheck
    .map(formatInstallCheck.bind(null, opts.cwd))
    .filter(Boolean)
    .map((msg) => ({msg}))
    .map(most.of) as most.Stream<most.Stream<{msg: string}>>

  outputs.push(installCheckOutput$)

  outputs.push(
    log$.skippedOptionalDependency
      .filter((log) => Boolean(log['prefix'] === opts.cwd && log.parents && log.parents.length === 0))
      .map((log) => most.of({
        msg: `info: ${
          log.package['id'] || log.package.name && (`${log.package.name}@${log.package.version}`) || log.package['pref']
        } is an optional dependency and failed compatibility check. Excluding it from installation.`,
      })),
  )

  const hookOutput$ = log$.hook
    .map((log) => {
      if (log['prefix'] === opts.cwd) {
        return {msg: `${chalk.magentaBright(log['hook'])}: ${log['message']}`}
      }
      return {
        msg: `${rightPad(formatPrefix(cwd, log['prefix']), PREFIX_MAX_LENGTH)} | ${chalk.magentaBright(log['hook'])}: ${log['message']}`,
      }
    })
    .map(most.of)

  outputs.push(hookOutput$)

  return outputs
}

function miscOutput (
  log$: most.Stream<supi.Log>,
  opts: {
    cwd: string,
  },
) {
  return log$
    .filter((obj) => obj.level !== 'debug' && (obj.level !== 'info' || !obj['prefix'] || obj['prefix'] === opts.cwd))
    .map((obj) => {
      switch (obj.level) {
        case 'warn':
          if (!obj.prefix || obj.prefix === opts.cwd)
            return formatWarn(obj.message)
          return `${rightPad(formatPrefix(opts.cwd, obj.prefix), PREFIX_MAX_LENGTH)} | ${formatWarn(obj.message)}`
        case 'error':
          if (obj['message'] && obj['message']['prefix'] && obj['message']['prefix'] !== opts.cwd) {
            return `${obj['message']['prefix']}:` + os.EOL + reportError(obj)
          }
          return reportError(obj)
        default:
          return obj['message']
      }
    })
    .map((msg) => ({msg}))
    .map(most.of)
}

function statsForCurrentPackage (
  stats$: most.Stream<supi.StatsLog>,
  opts: {
    cmd: string,
    currentPrefix: string,
    width: number,
  },
) {
  return most.fromPromise(
    stats$
      .filter((log) => log.prefix === opts.currentPrefix)
      .take((opts.cmd === 'install' || opts.cmd === 'update') ? 2 : 1)
      .reduce((acc, log) => {
        if (typeof log['added'] === 'number') {
          acc['added'] = log['added']
        } else if (typeof log['removed'] === 'number') {
          acc['removed'] = log['removed']
        }
        return acc
      }, {}),
  )
  .map((stats) => {
    if (!stats['removed'] && !stats['added']) {
      if (opts.cmd === 'link') {
        return most.never()
      }
      return most.of({msg: 'Already up-to-date'})
    }

    let msg = 'Packages:'
    if (stats['added']) {
      msg += ' ' + chalk.green(`+${stats['added']}`)
    }
    if (stats['removed']) {
      msg += ' ' + chalk.red(`-${stats['removed']}`)
    }
    msg += EOL + printPlusesAndMinuses(opts.width, (stats['added'] || 0), (stats['removed'] || 0))
    return most.of({msg})
  })
}

function statsForNotCurrentPackage (
  stats$: most.Stream<supi.StatsLog>,
  opts: {
    currentPrefix: string,
    subCmd?: string,
    width: number,
  },
) {
  const cookedStats$ = (
    opts.subCmd !== 'uninstall'
      ? stats$
          .loop((stats, log) => {
            // As of pnpm v2.9.0, during `pnpm recursive link`, logging of removed stats happens twice
            //  1. during linking
            //  2. during installing
            // Hence, the stats are added before reported
            if (!stats[log.prefix]) {
              stats[log.prefix] = log
              return {seed: stats, value: null}
            } else if (typeof stats[log.prefix].added === 'number' && typeof log['added'] === 'number') {
              stats[log.prefix].added += log['added']
              return {seed: stats, value: null}
            } else if (typeof stats[log.prefix].removed === 'number' && typeof log['removed'] === 'number') {
              stats[log.prefix].removed += log['removed']
              return {seed: stats, value: null}
            } else {
              const value = {...stats[log.prefix], ...log}
              delete stats[log.prefix]
              return {seed: stats, value}
            }
          }, {})
      : stats$
  )
  return cookedStats$
    .filter((stats) => stats !== null && (stats['removed'] || stats['added']))
    .map((stats) => {
      const prefix = formatPrefix(opts.currentPrefix, stats['prefix'])

      let msg = `${rightPad(prefix, PREFIX_MAX_LENGTH)} |`

      if (stats['added']) {
        msg += ` ${padStep(chalk.green(`+${stats['added']}`), 4)}`
      }
      if (stats['removed']) {
        msg += ` ${padStep(chalk.red(`-${stats['removed']}`), 4)}`
      }

      const rest = Math.max(0, opts.width - 1 - stringLength(msg))
      msg += ' ' + printPlusesAndMinuses(rest, roundStats(stats['added'] || 0), roundStats(stats['removed'] || 0))
      return most.of({msg})
    })
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

function formatPrefix (cwd: string, prefix: string) {
  prefix = normalize(path.relative(cwd, prefix) || '.')

  if (prefix.length <= PREFIX_MAX_LENGTH) {
    return prefix
  }

  const shortPrefix = prefix.substr(-PREFIX_MAX_LENGTH + 3)

  const separatorLocation = shortPrefix.indexOf('/')

  if (separatorLocation <= 0) {
    return `...${shortPrefix}`
  }

  return `...${shortPrefix.substr(separatorLocation)}`
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

function printDiffs (
  pkgsDiff: PackageDiff[],
  opts: {
    prefix: string,
  },
) {
  // Sorts by alphabet then by removed/added
  // + ava 0.10.0
  // - chalk 1.0.0
  // + chalk 2.0.0
  pkgsDiff.sort((a, b) => (a.name.localeCompare(b.name) * 10 + (Number(!b.added) - Number(!a.added))))
  const msg = pkgsDiff.map((pkg) => {
    let result = pkg.added
      ? ADDED_CHAR
      : REMOVED_CHAR
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
    if (pkg.from) {
      result += ` ${chalk.grey(`<- ${pkg.from && path.relative(opts.prefix, pkg.from) || '???'}`)}`
    }
    return result
  }).join(EOL)
  return msg
}

const ANSI_ESCAPES_LENGTH_OF_PREFIX = hlValue(' ').length - 1

function formatLifecycleHideOverflow (
  maxWidth: number,
  cwd: string,
  logObj: LifecycleLog,
) {
  const prefix = `${
    logObj.wd === logObj.depPath
      ? rightPad(formatPrefix(cwd, logObj.wd), PREFIX_MAX_LENGTH)
      : rightPad(logObj.depPath, PREFIX_MAX_LENGTH)
  } | ${hlValue(padStart(logObj.stage, 11))}`
  if (logObj['script']) {
    return `${prefix}$ ${logObj['script']}`
  }
  if (logObj['exitCode'] === 0) {
    return `${prefix}: done`
  }
  const maxLineWidth = maxWidth - prefix.length - 2 + ANSI_ESCAPES_LENGTH_OF_PREFIX
  const line = formatLine(maxLineWidth, logObj)
  return `${prefix}: ${line}`
}

function formatLine (maxWidth: number, logObj: LifecycleLog) {
  if (typeof logObj['exitCode'] === 'number') return chalk.red(`Exited with ${logObj['exitCode']}`)

  const line = stripAnsi(logObj['line']).substr(0, maxWidth)

  // TODO: strip only the non-color/style ansi escape codes
  if (logObj['stdio'] === 'stderr') {
    return chalk.gray(line)
  }
  return line
}

function formatInstallCheck (currentPrefix: string, logObj: InstallCheckLog) {
  const msg = currentPrefix === logObj['prefix']
    ? ''
    : `${rightPad(formatPrefix(currentPrefix, logObj['prefix']), PREFIX_MAX_LENGTH)} | `
  switch (logObj.code) {
    case 'EBADPLATFORM':
      return msg + formatWarn(`Unsupported system. Skipping dependency ${logObj.pkgId}`)
    case 'ENOTSUP':
      return msg + logObj.toString()
    default:
      return
  }
}

function formatWarn (message: string) {
  // The \u2009 is the "thin space" unicode character
  // It is used instead of ' ' because chalk (as of version 2.1.0)
  // trims whitespace at the beginning
  return `${chalk.bgYellow.black('\u2009WARN\u2009')} ${message}`
}
