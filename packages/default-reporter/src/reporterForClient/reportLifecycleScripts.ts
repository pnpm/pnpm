import { LifecycleLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import most = require('most')
import path = require('path')
import prettyTime = require('pretty-time')
import rightPad = require('right-pad')
import padStart = require('string.prototype.padstart')
import stripAnsi = require('strip-ansi')
import PushStream = require('zen-push')
import { EOL } from '../constants'
import {
  hlValue,
  PREFIX_MAX_LENGTH,
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
  const formatLifecycle = formatLifecycleHideOverflow.bind(null, opts.appendOnly ? Infinity : opts.width)
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
      let msg: string
      if (lifecycleMessages[key].collapsed) {
        lifecycleMessages[key]['label'] = lifecycleMessages[key]['label'] ||
          `${highlightLastFolder(formatPrefixNoTrim(opts.cwd, log.wd))}: Running ${log.stage} script`
        if (typeof log['exitCode'] === 'number') {
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
          delete lifecycleMessages[key]
        } else {
          lifecycleMessages[key].output.push(formatLifecycle(opts.cwd, log))
          msg = `${lifecycleMessages[key]['label']}...`
        }
      } else {
        if (log['script']) {
          lifecycleMessages[key].script = formatLifecycle(opts.cwd, log)
        } else {
          if (!lifecycleMessages[key].output.length || log['exitCode'] !== 0) {
            lifecycleMessages[key].output.push(formatLifecycle(opts.cwd, log))
          }
          if (lifecycleMessages[key].output.length > 3) {
            lifecycleMessages[key].output.shift()
          }
        }
        msg = EOL + [
          lifecycleMessages[key].script,
          ...lifecycleMessages[key].output,
        ].join(EOL)
      }
      if (!lifecycleStreamByDepPath[key]) {
        lifecycleStreamByDepPath[key] = new PushStream()
        lifecyclePushStream.next(most.from(lifecycleStreamByDepPath[key].observable))
      }
      lifecycleStreamByDepPath[key].next({ msg })
      if (typeof log['exitCode'] === 'number') {
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
