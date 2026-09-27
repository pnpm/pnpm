import { join as shellQuote } from 'shlex'
import which from 'which'

import { commandParsedByCmd } from './selectShell.js'

export interface QuoteScriptArgsOptions {
  platform: NodeJS.Platform
  scriptShell?: string
  shellEmulator?: boolean
  /** The `PATH` the script runs with. Read only when `cmd` parses the script. */
  searchPath: () => string
}

/**
 * Appends `args` to `script` so that the script's command receives each of
 * them unchanged, quoted for whichever shell will parse the script.
 */
export function appendScriptArgs (script: string, args: string[], opts: QuoteScriptArgsOptions): string {
  if (args.length === 0) return script
  if (!commandParsedByCmd(opts.scriptShell, opts.platform, opts.shellEmulator)) {
    return `${script} ${shellQuote(args)}`
  }
  const doubleEscape = isBatchFile(firstWord(script), opts.searchPath())
  return `${script} ${args.map((arg) => quoteForCmd(arg, doubleEscape)).join(' ')}`
}

/**
 * Quotes `arg` for a `cmd /d /s /c` command line the way npm does. The
 * argument is first quoted for the target program's C runtime, then every
 * character `cmd` would interpret is escaped with `^`. A batch file parses
 * its arguments a second time, so `doubleEscape` escapes them twice.
 *
 * `cmd` ends a command line at a line break, so line breaks are passed as
 * the two characters `\n` or `\r`.
 */
export function quoteForCmd (arg: string, doubleEscape: boolean): string {
  if (arg === '') return '""'
  arg = arg.replaceAll('\r', '\\r').replaceAll('\n', '\\n')
  let quoted = /[ \t\v"]/.test(arg) ? quoteForCRuntime(arg) : arg
  quoted = quoted.replace(CMD_META_CHARS, '^$&')
  if (doubleEscape) {
    quoted = quoted.replace(CMD_META_CHARS, '^$&')
  }
  return quoted
}

const CMD_META_CHARS = /[ !%^&()<>|"]/g

// https://learn.microsoft.com/en-us/archive/blogs/twistylittlepassagesallalike/everyone-quotes-command-line-arguments-the-wrong-way
function quoteForCRuntime (arg: string): string {
  let quoted = '"'
  let backslashes = 0
  for (const char of arg) {
    if (char === '\\') {
      backslashes++
      continue
    }
    quoted += '\\'.repeat(char === '"' ? backslashes * 2 + 1 : backslashes) + char
    backslashes = 0
  }
  return `${quoted}${'\\'.repeat(backslashes * 2)}"`
}

function firstWord (script: string): string {
  script = script.trimStart()
  let insideQuotes = false
  let end = 0
  while (end < script.length && (insideQuotes || script[end] !== ' ')) {
    if (script[end] === '"') insideQuotes = !insideQuotes
    end++
  }
  return script.slice(0, end).replaceAll('"', '')
}

function isBatchFile (command: string, searchPath: string): boolean {
  const resolved = (which.sync(command, { path: searchPath, nothrow: true }) ?? command).toLowerCase()
  return resolved.endsWith('.cmd') || resolved.endsWith('.bat')
}
