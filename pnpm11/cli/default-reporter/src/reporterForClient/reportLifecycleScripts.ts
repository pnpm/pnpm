import path from 'node:path'
import { stripVTControlCharacters } from 'node:util'

import type { LifecycleLog } from '@pnpm/core-loggers'
import type { LogLevel } from '@pnpm/logger'
import chalk from 'chalk'
import cliTruncate from 'cli-truncate'
import prettyTime from 'pretty-ms'
import * as Rx from 'rxjs'
import { buffer, filter, groupBy, map, mergeMap } from 'rxjs/operators'

import { EOL } from '../constants.js'
import { hlValue } from './outputConstants.js'
import { formatPrefix, formatPrefixNoTrim } from './utils/formatPrefix.js'

const NODE_MODULES = `${path.sep}node_modules${path.sep}`
const TMP_DIR_IN_STORE = `tmp${path.sep}_tmp_` // git-hosted dependencies are built in these temporary directories

// When streaming processes are spawned, use this color for prefix
const colorWheel = ['cyan', 'magenta', 'blue', 'yellow', 'green', 'red'] as const
const NUM_COLORS = colorWheel.length

// Ever-increasing index ensures colors are always sequential
let currentColor = 0

type ColorByPkg = Map<string, (txt: string) => string>

export function reportLifecycleScripts (
  log$: {
    lifecycle: Rx.Observable<LifecycleLog>
  },
  opts: {
    appendOnly?: boolean
    aggregateOutput?: boolean
    hideLifecyclePrefix?: boolean
    logLevel?: LogLevel
    annotateOptionalFailure?: boolean
    cwd: string
    width: number
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  // When the reporter is not append-only, the length of output is limited
  // in order to reduce flickering
  if (opts.appendOnly) {
    return streamLifecycleScripts(log$.lifecycle, opts)
  }
  return renderLifecycleScripts(log$.lifecycle, opts)
}

function streamLifecycleScripts (
  lifecycle$: Rx.Observable<LifecycleLog>,
  opts: {
    aggregateOutput?: boolean
    hideLifecyclePrefix?: boolean
    logLevel?: LogLevel
    annotateOptionalFailure?: boolean
    cwd: string
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  if (opts.aggregateOutput) {
    lifecycle$ = lifecycle$.pipe(aggregateOutput(opts.logLevel))
  }

  const streamLifecycleOutput = createStreamLifecycleOutput(opts.cwd, !!opts.hideLifecyclePrefix, opts.annotateOptionalFailure)
  return lifecycle$.pipe(
    map((log: LifecycleLog) => Rx.of({
      msg: streamLifecycleOutput(log),
    }))
  )
}

interface LifecycleMessageCache {
  collapsed: boolean
  output: string[]
  script: string
  startTime: [number, number]
  status: string
}

interface LifecycleOutputStreams {
  lifecycleStreamByDepPath: Record<string, Rx.Subject<{ msg: string }>>
  lifecyclePushStream: Rx.Subject<Rx.Observable<{ msg: string }>>
}

function renderLifecycleScripts (
  lifecycle$: Rx.Observable<LifecycleLog>,
  opts: { cwd: string, width: number }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const lifecycleMessages: Record<string, LifecycleMessageCache> = {}
  const streams: LifecycleOutputStreams = {
    lifecycleStreamByDepPath: {},
    lifecyclePushStream: new Rx.Subject<Rx.Observable<{ msg: string }>>(),
  }

  void lifecycle$
    .forEach((log: LifecycleLog) => {
      const key = `${log.stage}:${log.depPath}`
      const exit = typeof log['exitCode'] === 'number'
      const msg = renderLifecycleMessage(lifecycleMessages, key, log, { cwd: opts.cwd, exit, maxWidth: opts.width })
      pushLifecycleMessage(streams, key, { msg, exit })
    })

  return Rx.from(streams.lifecyclePushStream)
}

function renderLifecycleMessage (
  lifecycleMessages: Record<string, LifecycleMessageCache>,
  key: string,
  log: LifecycleLog,
  opts: { cwd: string, exit: boolean, maxWidth: number }
): string {
  lifecycleMessages[key] = lifecycleMessages[key] || {
    collapsed: log.wd.includes(NODE_MODULES) || log.wd.includes(TMP_DIR_IN_STORE),
    output: [],
    startTime: process.hrtime(),
    status: formatIndentedStatus(chalk.magentaBright('Running...')),
  }
  const msg = lifecycleMessages[key].collapsed
    ? renderCollapsedScriptOutput(log, lifecycleMessages[key], opts)
    : renderScriptOutput(log, lifecycleMessages[key], opts)
  if (opts.exit) {
    delete lifecycleMessages[key]
  }
  return msg
}

function pushLifecycleMessage (
  { lifecycleStreamByDepPath, lifecyclePushStream }: LifecycleOutputStreams,
  key: string,
  { msg, exit }: { msg: string, exit: boolean }
): void {
  if (!lifecycleStreamByDepPath[key]) {
    lifecycleStreamByDepPath[key] = new Rx.Subject<{ msg: string }>()
    lifecyclePushStream.next(Rx.from(lifecycleStreamByDepPath[key]))
  }
  lifecycleStreamByDepPath[key].next({ msg })
  if (exit) {
    lifecycleStreamByDepPath[key].complete()
  }
}

function toNano (time: [number, number]): number {
  return (time[0] + (time[1] / 1e9)) * 1e3
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
): string {
  if (!messageCache.label) {
    messageCache.label = highlightLastFolder(formatPrefixNoTrim(opts.cwd, log.wd))
    if (log.wd.includes(TMP_DIR_IN_STORE)) {
      messageCache.label += ` [${log.depPath}]`
    }
    messageCache.label += `: Running ${log.stage} script`
  }
  if (!opts.exit) {
    updateMessageCache(log, messageCache, opts)
    return `${messageCache.label}...`
  }
  const time = prettyTime(toNano(process.hrtime(messageCache.startTime)))
  if (log.exitCode === 0) {
    return `${messageCache.label}, done in ${time}`
  }
  if (log.optional === true) {
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
): string {
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
): void {
  if (log.script) {
    const prefix = `${formatPrefix(opts.cwd, log.wd)} ${hlValue(log.stage)}`
    const maxLineWidth = opts.maxWidth - prefix.length - 2 + ANSI_ESCAPES_LENGTH_OF_PREFIX
    messageCache.script = `${prefix}$ ${cutLine(log.script, maxLineWidth)}`
  } else if (opts.exit) {
    const time = prettyTime(toNano(process.hrtime(messageCache.startTime)))
    if (log.exitCode === 0) {
      messageCache.status = formatIndentedStatus(chalk.magentaBright(`Done in ${time}`))
    } else {
      messageCache.status = formatIndentedStatus(chalk.red(`Failed in ${time} at ${log.wd}`))
    }
  } else {
    messageCache.output.push(formatIndentedOutput(opts.maxWidth, log))
  }
}

function formatIndentedStatus (status: string): string {
  return `${chalk.magentaBright('└─')} ${status}`
}

function highlightLastFolder (dir: string): string {
  const lastSlash = dir.lastIndexOf('/') + 1
  return `${chalk.gray(dir.slice(0, lastSlash))}${dir.slice(lastSlash)}`
}

const ANSI_ESCAPES_LENGTH_OF_PREFIX = hlValue(' ').length - 1

function createStreamLifecycleOutput (cwd: string, hideLifecyclePrefix: boolean, annotateOptionalFailure = false): (logObj: LifecycleLog) => string {
  currentColor = 0
  const colorByPrefix: ColorByPkg = new Map()
  return streamLifecycleOutput.bind(null, colorByPrefix, cwd, hideLifecyclePrefix, annotateOptionalFailure)
}

function streamLifecycleOutput (
  colorByPkg: ColorByPkg,
  cwd: string,
  hideLifecyclePrefix: boolean,
  annotateOptionalFailure: boolean,
  logObj: LifecycleLog
): string {
  const prefix = formatLifecycleScriptPrefix(colorByPkg, cwd, logObj.wd, logObj.stage)
  if (typeof logObj.exitCode === 'number') {
    if (logObj.exitCode === 0) {
      return `${prefix}: Done`
    } else {
      return `${prefix}: Failed${annotateOptionalFailure && logObj.optional ? ' (skipped as optional)' : ''}`
    }
  }
  if (logObj['script']) {
    return `${prefix}$ ${logObj['script'] as string}`
  }
  const line = formatLine(Infinity, logObj)
  return hideLifecyclePrefix ? line : `${prefix}: ${line}`
}

function formatIndentedOutput (maxWidth: number, logObj: LifecycleLog): string {
  return `${chalk.magentaBright('│')} ${formatLine(maxWidth - 2, logObj)}`
}

function formatLifecycleScriptPrefix (
  colorByPkg: ColorByPkg,
  cwd: string,
  wd: string,
  stage: string
): string {
  if (!colorByPkg.has(wd)) {
    const colorName = colorWheel[currentColor % NUM_COLORS]
    colorByPkg.set(wd, chalk[colorName])
    currentColor += 1
  }

  const color = colorByPkg.get(wd)!
  return `${color(formatPrefix(cwd, wd))} ${hlValue(stage)}`
}

function formatLine (maxWidth: number, logObj: LifecycleLog): string {
  const line = cutLine(logObj.line && printableScriptLine(logObj.line), maxWidth)

  if (logObj.stdio === 'stderr') {
    return chalk.gray(line)
  }
  return line
}

// eslint-disable-next-line no-control-regex -- an SGR (color) sequence starts with ESC
const SGR_SEQUENCE = /\u001B\[[\d;:]*m/g
// eslint-disable-next-line no-control-regex -- C1 DCS, SOS, OSC, PM and APC strings, which stripVTControlCharacters does not recognize
const C1_CONTROL_STRING = /[\u0090\u0098\u009D-\u009F][^\u0007\u001B\u009C]*(?:\u0007|\u009C|\u001B\\)?/g
// eslint-disable-next-line no-control-regex -- matching control characters is the point of this pattern
const CONTROL_CHARACTERS_EXCEPT_TAB = /[\u0000-\u0008\u000A-\u001F\u007F-\u009F]/g

const SGR_RESET = '\u001B[0m'

/**
 * Keeps the SGR (color) sequences of a script's output line when colors are
 * on, followed by a reset so a color left open cannot reach the next line.
 * Drops every other escape sequence and control character, so a script's
 * cursor movement cannot move the reporter's cursor.
 */
function printableScriptLine (line: string): string {
  const colors = chalk.level > 0 ? line.match(SGR_SEQUENCE) ?? [] : []
  const printable = line.split(SGR_SEQUENCE)
    .map((text, index) => stripVTControlCharacters(text.replace(C1_CONTROL_STRING, '')).replace(CONTROL_CHARACTERS_EXCEPT_TAB, '') + (colors[index] ?? ''))
    .join('')
  return colors.length > 0 ? printable + SGR_RESET : printable
}

function cutLine (line: string | undefined, maxLength: number): string {
  if (!line) return ''
  // Streamed lifecycle output is printed in full (maxLength is Infinity).
  // cli-truncate rejects a non-finite width, so skip truncation in that case.
  if (!Number.isFinite(maxLength)) return line
  return cliTruncate(line, maxLength)
}

function aggregateOutput (logLevel: LogLevel | undefined): (source: Rx.Observable<LifecycleLog>) => Rx.Observable<LifecycleLog> {
  return (source) => source.pipe(
    // `pnpm -r exec` reports a project's manifest name as its depPath, so
    // same-named projects differ only by wd.
    groupBy((data) => `${data.depPath}\0${data.stage}\0${data.wd}`),
    mergeMap(group => group.pipe(
      buffer(group.pipe(filter(msg => 'exitCode' in msg))),
      filter((messages) => shouldPrintAggregatedOutput(messages, logLevel)),
      mergeMap((messages) => Rx.from(messages))
    ))
  )
}

function shouldPrintAggregatedOutput (messages: LifecycleLog[], logLevel: LogLevel | undefined): boolean {
  if (logLevel == null || logLevel === 'info' || logLevel === 'debug') return true
  const exit = messages.at(-1)
  return exit != null && 'exitCode' in exit && exit.exitCode !== 0 && !(exit.optional === true && logLevel === 'error')
}
