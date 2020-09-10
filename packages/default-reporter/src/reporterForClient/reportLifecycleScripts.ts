import { LifecycleLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import { EOL } from '../constants'
import formatPrefix, { formatPrefixNoTrim } from './utils/formatPrefix'
import {
  hlValue,
} from './outputConstants'
import chalk = require('chalk')
import path = require('path')
import prettyTime = require('pretty-time')
import stripAnsi = require('strip-ansi')

const NODE_MODULES = `${path.sep}node_modules${path.sep}`

// When streaming processes are spawned, use this color for prefix
const colorWheel = ['cyan', 'magenta', 'blue', 'yellow', 'green', 'red']
const NUM_COLORS = colorWheel.length

// Ever-increasing index ensures colors are always sequential
let currentColor = 0

type ColorByPkg = Map<string, (txt: string) => string>

export default (
  log$: {
    lifecycle: Rx.Observable<LifecycleLog>
  },
  opts: {
    appendOnly?: boolean
    cwd: string
    width: number
  }
) => {
  // When the reporter is not append-only, the length of output is limited
  // in order to reduce flickering
  if (opts.appendOnly) {
    const streamLifecycleOutput = createStreamLifecycleOutput(opts.cwd)
    return log$.lifecycle.pipe(
      map((log: LifecycleLog) => Rx.of({
        msg: streamLifecycleOutput(log),
      }))
    )
  }
  const lifecycleMessages: {
    [depPath: string]: {
      collapsed: boolean
      output: string[]
      script: string
      startTime: [number, number]
      status: string
    }
  } = {}
  const lifecycleStreamByDepPath: {
    [depPath: string]: Rx.Subject<{ msg: string }>
  } = {}
  const lifecyclePushStream = new Rx.Subject<Rx.Observable<{ msg: string }>>()

  // TODO: handle promise of .forEach?!
  log$.lifecycle // eslint-disable-line
    .forEach((log: LifecycleLog) => {
      const key = `${log.stage}:${log.depPath}`
      lifecycleMessages[key] = lifecycleMessages[key] || {
        collapsed: log.wd.includes(NODE_MODULES),
        output: [],
        startTime: process.hrtime(),
        status: formatIndentedStatus(chalk.magentaBright('Running...')),
      }
      const exit = typeof log['exitCode'] === 'number'
      let msg: string
      if (lifecycleMessages[key].collapsed) {
        msg = renderCollapsedScriptOutput(log, lifecycleMessages[key], { cwd: opts.cwd, exit, maxWidth: opts.width })
      } else {
        msg = renderScriptOutput(log, lifecycleMessages[key], { cwd: opts.cwd, exit, maxWidth: opts.width })
      }
      if (exit) {
        delete lifecycleMessages[key]
      }
      if (!lifecycleStreamByDepPath[key]) {
        lifecycleStreamByDepPath[key] = new Rx.Subject<{ msg: string }>()
        lifecyclePushStream.next(Rx.from(lifecycleStreamByDepPath[key]))
      }
      lifecycleStreamByDepPath[key].next({ msg })
      if (exit) {
        lifecycleStreamByDepPath[key].complete()
      }
    })

  return Rx.from(lifecyclePushStream)
}

function renderCollapsedScriptOutput (
  log: LifecycleLog,
  messageCache: {
    collapsed: boolean
    label?: string
    output: string[]
    script: string
    startTime: [number, number]
    status: string
  },
  opts: {
    cwd: string
    exit: boolean
    maxWidth: number
  }
) {
  messageCache.label = messageCache.label ??
    `${highlightLastFolder(formatPrefixNoTrim(opts.cwd, log.wd))}: Running ${log.stage} script`
  if (!opts.exit) {
    updateMessageCache(log, messageCache, opts)
    return `${messageCache.label}...`
  }
  const time = prettyTime(process.hrtime(messageCache.startTime))
  if (log['exitCode'] === 0) {
    return `${messageCache.label}, done in ${time}`
  }
  if (log['optional'] === true) {
    return `${messageCache.label}, failed in ${time} (skipped as optional)`
  }
  return `${messageCache.label}, failed in ${time}${EOL}${renderScriptOutput(log, messageCache, opts)}`
}

function renderScriptOutput (
  log: LifecycleLog,
  messageCache: {
    collapsed: boolean
    output: string[]
    script: string
    startTime: [number, number]
    status: string
  },
  opts: {
    cwd: string
    exit: boolean
    maxWidth: number
  }
) {
  updateMessageCache(log, messageCache, opts)
  if (opts.exit && log['exitCode'] !== 0) {
    return [
      messageCache.script,
      ...messageCache.output,
      messageCache.status,
    ].join(EOL)
  }
  if (messageCache.output.length > 10) {
    return [
      messageCache.script,
      `[${messageCache.output.length - 10} lines collapsed]`,
      ...messageCache.output.slice(messageCache.output.length - 10),
      messageCache.status,
    ].join(EOL)
  }
  return [
    messageCache.script,
    ...messageCache.output,
    messageCache.status,
  ].join(EOL)
}

function updateMessageCache (
  log: LifecycleLog,
  messageCache: {
    collapsed: boolean
    output: string[]
    script: string
    startTime: [number, number]
    status: string
  },
  opts: {
    cwd: string
    exit: boolean
    maxWidth: number
  }
) {
  if (log['script']) {
    const prefix = `${formatPrefix(opts.cwd, log.wd)} ${hlValue(log.stage)}`
    const maxLineWidth = opts.maxWidth - prefix.length - 2 + ANSI_ESCAPES_LENGTH_OF_PREFIX
    messageCache.script = `${prefix}$ ${cutLine(log['script'], maxLineWidth)}`
  } else if (opts.exit) {
    const time = prettyTime(process.hrtime(messageCache.startTime))
    if (log['exitCode'] === 0) {
      messageCache.status = formatIndentedStatus(chalk.magentaBright(`Done in ${time}`))
    } else {
      messageCache.status = formatIndentedStatus(chalk.red(`Failed in ${time}`))
    }
  } else {
    messageCache.output.push(formatIndentedOutput(opts.maxWidth, log))
  }
}

function formatIndentedStatus (status: string) {
  return `${chalk.magentaBright('└─')} ${status}`
}

function highlightLastFolder (p: string) {
  const lastSlash = p.lastIndexOf('/') + 1
  return `${chalk.gray(p.substr(0, lastSlash))}${p.substr(lastSlash)}`
}

const ANSI_ESCAPES_LENGTH_OF_PREFIX = hlValue(' ').length - 1

function createStreamLifecycleOutput (cwd: string) {
  currentColor = 0
  const colorByPrefix: ColorByPkg = new Map()
  return streamLifecycleOutput.bind(null, colorByPrefix, cwd)
}

function streamLifecycleOutput (
  colorByPkg: ColorByPkg,
  cwd: string,
  logObj: LifecycleLog
) {
  const prefix = formatLifecycleScriptPrefix(colorByPkg, cwd, logObj.wd, logObj.stage)
  if (typeof logObj['exitCode'] === 'number') {
    if (logObj['exitCode'] === 0) {
      return `${prefix}: Done`
    } else {
      return `${prefix}: Failed`
    }
  }
  if (logObj['script']) {
    return `${prefix}$ ${logObj['script'] as string}`
  }
  const line = formatLine(Infinity, logObj)
  return `${prefix}: ${line}`
}

function formatIndentedOutput (maxWidth: number, logObj: LifecycleLog) {
  return `${chalk.magentaBright('│')} ${formatLine(maxWidth - 2, logObj)}`
}

function formatLifecycleScriptPrefix (
  colorByPkg: ColorByPkg,
  cwd: string,
  wd: string,
  stage: string
) {
  if (!colorByPkg.has(wd)) {
    const colorName = colorWheel[currentColor % NUM_COLORS]
    colorByPkg.set(wd, chalk[colorName])
    currentColor += 1
  }

  const color = colorByPkg.get(wd)!
  return `${color(formatPrefix(cwd, wd))} ${hlValue(stage)}`
}

function formatLine (maxWidth: number, logObj: LifecycleLog) {
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
