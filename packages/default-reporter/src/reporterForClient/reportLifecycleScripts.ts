import { LifecycleLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import most = require('most')
import path = require('path')
import prettyTime = require('pretty-time')
import stripAnsi = require('strip-ansi')
import PushStream = require('zen-push')
import { EOL } from '../constants'
import {
  hlValue,
} from './outputConstants'
import formatPrefix, { formatPrefixNoTrim } from './utils/formatPrefix'

const NODE_MODULES = `${path.sep}node_modules${path.sep}`

export default (
  log$: {
    lifecycle: most.Stream<LifecycleLog>,
  },
  opts: {
    appendOnly?: boolean,
    cwd: string,
    width: number,
  },
) => {
// When the reporter is not append-only, the length of output is limited
  // in order to reduce flickering
  const formatLifecycle = opts.appendOnly
    ? formatLifecycleHideOverflowForAppendOnly
    : formatLifecycleHideOverflow.bind(null, opts.width)
  if (opts.appendOnly) {
    return most.of(
      log$.lifecycle
        .map((log: LifecycleLog) => ({ msg: formatLifecycle(opts.cwd, log) })),
    )
  }
  const lifecycleMessages: {
    [depPath: string]: {
      collapsed: boolean,
      output: string[],
      script: string,
      startTime: [number, number],
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

  // TODO: handle promise of .forEach?!
  log$.lifecycle // tslint:disable-line
    .forEach((log: LifecycleLog) => {
      const key = `${log.stage}:${log.depPath}`
      lifecycleMessages[key] = lifecycleMessages[key] || {
        collapsed: log.wd.includes(NODE_MODULES),
        output: [],
        startTime: process.hrtime(),
      }
      const exit = typeof log['exitCode'] === 'number'
      let msg: string
      if (lifecycleMessages[key].collapsed) {
        lifecycleMessages[key]['label'] = lifecycleMessages[key]['label'] ||
          `${highlightLastFolder(formatPrefixNoTrim(opts.cwd, log.wd))}: Running ${log.stage} script`
        if (exit) {
          const time = prettyTime(process.hrtime(lifecycleMessages[key].startTime))
          if (log['exitCode'] === 0) {
            msg = `${lifecycleMessages[key]['label']}, done in ${time}`
          } else if (log['optional'] === true) {
            msg = `${lifecycleMessages[key]['label']}, failed in ${time} (skipped as optional)`
          } else {
            msg = [
              `${lifecycleMessages[key]['label']}, failed in ${time}`,
              ...lifecycleMessages[key].output,
            ].join(EOL)
          }
        } else {
          lifecycleMessages[key].output.push(formatLifecycle(opts.cwd, log))
          msg = `${lifecycleMessages[key]['label']}...`
        }
      } else {
        if (log['script']) {
          lifecycleMessages[key].script = formatLifecycle(opts.cwd, log)
        } else {
          lifecycleMessages[key].output.push(formatLifecycle(opts.cwd, log))
        }
        msg = EOL + [
          lifecycleMessages[key].script,
          ...(
            exit && log['exitCode'] !== 0
              ? lifecycleMessages[key].output
              : lifecycleMessages[key].output.slice(lifecycleMessages[key].output.length - 10)
          ),
        ].join(EOL)
      }
      if (exit) {
        delete lifecycleMessages[key]
      }
      if (!lifecycleStreamByDepPath[key]) {
        lifecycleStreamByDepPath[key] = new PushStream()
        lifecyclePushStream.next(most.from(lifecycleStreamByDepPath[key].observable))
      }
      lifecycleStreamByDepPath[key].next({ msg })
      if (exit) {
        lifecycleStreamByDepPath[key].complete()
      }
    })

  return most.from(lifecyclePushStream.observable) as most.Stream<most.Stream<{ msg: string }>>
}

function highlightLastFolder (p: string) {
  const lastSlash = p.lastIndexOf('/') + 1
  return `${chalk.gray(p.substr(0, lastSlash))}${p.substr(lastSlash)}`
}

const ANSI_ESCAPES_LENGTH_OF_PREFIX = hlValue(' ').length - 1

function formatLifecycleHideOverflowForAppendOnly (
  cwd: string,
  logObj: LifecycleLog,
) {
  const prefix = `${formatPrefix(cwd, logObj.wd)} ${hlValue(logObj.stage)}`
  if (logObj['exitCode'] === 0) {
    return `${prefix}: Done`
  }
  if (logObj['script']) {
    return `${prefix}$ ${logObj['script']}`
  }
  const line = formatLine(Infinity, logObj)
  return `${prefix}: ${line}`
}

function formatLifecycleHideOverflow (
  maxWidth: number,
  cwd: string,
  logObj: LifecycleLog,
) {
  const prefix = `${formatPrefix(cwd, logObj.wd)} ${hlValue(logObj.stage)}`
  const maxLineWidth = maxWidth - prefix.length - 2 + ANSI_ESCAPES_LENGTH_OF_PREFIX
  if (logObj['exitCode'] === 0) {
    return `${prefix}: Done`
  }
  if (logObj['script']) {
    return `${prefix}$ ${cutLine(logObj['script'], maxLineWidth)}`
  }
  return `${chalk.magentaBright('|')} ${formatLine(maxWidth - 2, logObj)}`
}

function formatLine (maxWidth: number, logObj: LifecycleLog) {
  if (typeof logObj['exitCode'] === 'number') return chalk.red(`Exited with ${logObj['exitCode']}`)

  const line = cutLine(logObj['line'], maxWidth)

  // TODO: strip only the non-color/style ansi escape codes
  if (logObj['stdio'] === 'stderr') {
    return chalk.gray(line)
  }
  return line
}

function cutLine (line: string, maxLength: number) {
  return stripAnsi(line).substr(0, maxLength)
}
