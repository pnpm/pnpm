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
  if (opts.appendOnly) {
    return most.of(
      log$.lifecycle
        .map((log: LifecycleLog) => ({ msg: formatLifecycleHideOverflowForAppendOnly(opts.cwd, log) })),
    )
  }
  const lifecycleMessages: {
    [depPath: string]: {
      collapsed: boolean,
      output: string[],
      script: string,
      startTime: [number, number],
      status: string,
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
  const formatLifecycle = formatLifecycleHideOverflow.bind(null, opts.width, opts.cwd)

  // TODO: handle promise of .forEach?!
  log$.lifecycle // tslint:disable-line
    .forEach((log: LifecycleLog) => {
      const key = `${log.stage}:${log.depPath}`
      lifecycleMessages[key] = lifecycleMessages[key] || {
        collapsed: log.wd.includes(NODE_MODULES),
        output: [],
        startTime: process.hrtime(),
        status: 'Running...',
      }
      const exit = typeof log['exitCode'] === 'number'
      let msg: string
      if (lifecycleMessages[key].collapsed) {
        msg = renderCollapsedScriptOutput(formatLifecycle, log, lifecycleMessages[key], { cwd: opts.cwd, exit })
      } else {
        msg = renderScriptOutput(formatLifecycle, log, lifecycleMessages[key], { cwd: opts.cwd, exit })
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

function renderCollapsedScriptOutput (
  formatLifecycle: (log: LifecycleLog) => string,
  log: LifecycleLog,
  messageCache: {
    collapsed: boolean,
    output: string[],
    script: string,
    startTime: [number, number],
    status: string,
  },
  opts: {
    cwd: string,
    exit: boolean,
  },
) {
  messageCache['label'] = messageCache['label'] ||
  `${highlightLastFolder(formatPrefixNoTrim(opts.cwd, log.wd))}: Running ${log.stage} script`
  if (!opts.exit) {
    messageCache.output.push(formatLifecycle(log))
    return `${messageCache['label']}...`
  }
  const time = prettyTime(process.hrtime(messageCache.startTime))
  if (log['exitCode'] === 0) {
    return `${messageCache['label']}, done in ${time}`
  }
  if (log['optional'] === true) {
    return `${messageCache['label']}, failed in ${time} (skipped as optional)`
  }
  return [
    `${messageCache['label']}, failed in ${time}`,
    ...messageCache.output,
  ].join(EOL)
}

function renderScriptOutput (
  formatLifecycle: (log: LifecycleLog) => string,
  log: LifecycleLog,
  messageCache: {
    collapsed: boolean,
    output: string[],
    script: string,
    startTime: [number, number],
    status: string,
  },
  opts: {
    cwd: string,
    exit: boolean,
  },
) {
  if (log['script']) {
    messageCache.script = formatLifecycle(log)
  } else if (opts.exit) {
    messageCache.status = formatLifecycle(log)
  } else {
    messageCache.output.push(formatLifecycle(log))
  }
  if (opts.exit && log['exitCode'] !== 0) {
    return EOL + [
      messageCache.script,
      ...messageCache.output,
      messageCache.status,
    ].join(EOL)
  }
  if (messageCache.output.length > 10) {
    return EOL + [
      messageCache.script,
      `[${messageCache.output.length - 10} lines collapsed]`,
      ...messageCache.output.slice(messageCache.output.length - 10),
      messageCache.status,
    ].join(EOL)
  }
  return EOL + [
    messageCache.script,
    ...messageCache.output,
    messageCache.status,
  ].join(EOL)
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
  const prefix = formatLifecycleScriptPrefix(cwd, logObj.wd, logObj.stage)
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
  const prefix = formatLifecycleScriptPrefix(cwd, logObj.wd, logObj.stage)
  const maxLineWidth = maxWidth - prefix.length - 2 + ANSI_ESCAPES_LENGTH_OF_PREFIX
  if (logObj['exitCode'] === 0) {
    return `${prefix}: Done`
  }
  if (logObj['script']) {
    return `${prefix}$ ${cutLine(logObj['script'], maxLineWidth)}`
  }
  return `${chalk.magentaBright('|')} ${formatLine(maxWidth - 2, logObj)}`
}

function formatLifecycleScriptPrefix (cwd: string, wd: string, stage: string) {
  return `${formatPrefix(cwd, wd)} ${hlValue(stage)}`
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
