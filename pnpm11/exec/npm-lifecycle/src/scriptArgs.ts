import path from 'node:path'

import { join as shellQuote } from 'shlex'
import which from 'which'

import { commandParsedByCmd } from './selectShell.js'

export interface QuoteScriptArgsOptions {
  platform: NodeJS.Platform
  scriptShell?: string
  shellEmulator?: boolean
  /** The directory the script runs in, which `cmd` searches before `PATH`. */
  wd: string
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
    return showScriptWithArgs(script, args)
  }
  const doubleEscape = isBatchFile(firstWord(lastCommand(script)), opts)
  return `${script} ${args.map((arg) => quoteForCmd(arg, doubleEscape)).join(' ')}`
}

/**
 * `script` with `args` quoted the POSIX way, which is how pnpm prints a script
 * on every platform. It keeps `cmd`'s `^` escapes out of the output.
 */
export function showScriptWithArgs (script: string, args: string[]): string {
  return args.length === 0 ? script : `${script} ${shellQuote(args)}`
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

/**
 * The last command of a chain such as `a && b | c`, which is the one that
 * receives the appended arguments.
 */
function lastCommand (script: string): string {
  let insideQuotes = false
  // An `&` right after an unescaped `>` or `<` duplicates a handle, as in `2>&1`.
  let redirecting = false
  let start = 0
  for (let charIndex = 0; charIndex < script.length; charIndex++) {
    const char = script[charIndex]
    const afterRedirection = redirecting
    redirecting = false
    if (char === '"') {
      insideQuotes = !insideQuotes
    } else if (insideQuotes) {
      continue
    } else if (char === '^') {
      charIndex++
    } else if (char === '>' || char === '<') {
      redirecting = true
    } else if (char === '|' || (char === '&' && !afterRedirection)) {
      start = charIndex + 1
    }
  }
  return script.slice(start)
}

function firstWord (command: string): string {
  command = command.trimStart()
  let insideQuotes = false
  let end = 0
  while (end < command.length && (insideQuotes || !CMD_SEPARATORS.has(command[end]))) {
    if (command[end] === '"') insideQuotes = !insideQuotes
    end++
  }
  return command.slice(0, end).replaceAll('"', '')
}

const CMD_SEPARATORS = new Set([' ', '\t'])

function isBatchFile (command: string, opts: Pick<QuoteScriptArgsOptions, 'wd' | 'searchPath'>): boolean {
  const resolved = /[\\/]/.test(command)
    ? which.sync(path.resolve(opts.wd, command), { nothrow: true })
    : which.sync(command, { path: [opts.wd, opts.searchPath()].join(path.delimiter), nothrow: true })
  const name = (resolved ?? command).toLowerCase()
  return name.endsWith('.cmd') || name.endsWith('.bat')
}
