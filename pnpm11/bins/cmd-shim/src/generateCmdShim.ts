import path from 'node:path'

import type { InternalOptions } from './types.js'
import { cmdEscape, isAscii, normalizePathEnvVar } from './utils.js'

/**
 * Generate the content of a shim for CMD.
 *
 * @param src Path to the executable or script.
 * @param to Path to the shim to be created.
 * It is highly recommended to end with `.cmd` (or `.bat`).
 */
export function generateCmdShim (src: string, to: string, opts: InternalOptions): string {
  const shTarget = path.relative(path.dirname(to), src)
  const target = cmdEscape(shTarget.split('/').join('\\'))
  const quotedPathToTarget = path.isAbsolute(target) ? `"${target}"` : `"%~dp0\\${target}"`
  const { prog, target: resolvedTarget, longProg } = resolveCmdProgram(opts, quotedPathToTarget)
  const args = opts.prog ? cmdEscape(opts.args || '') : ''
  const progArgs = opts.progArgs ? `${opts.progArgs.join(' ')} ` : ''

  let cmd = '@SETLOCAL\r\n'
  cmd += buildCmdEnvBlock(opts)
  cmd += buildCmdInvocation({
    args,
    longProg,
    prog,
    progArgs,
    target: resolvedTarget,
  })

  return withUtf8Codepage(cmd)
}

function resolveCmdProgram (opts: InternalOptions, quotedPathToTarget: string): {
  prog: string
  target: string
  longProg?: string
} {
  if (!opts.prog) {
    return {
      prog: quotedPathToTarget,
      target: '',
    }
  }
  if (opts.prog === 'node' && opts.nodeExecPath) {
    return {
      prog: `"${cmdEscape(opts.nodeExecPath)}"`,
      target: quotedPathToTarget,
    }
  }
  const prog = cmdEscape(opts.prog)
  return {
    longProg: `"%~dp0\\${prog}.exe"`,
    prog,
    target: quotedPathToTarget,
  }
}

function buildCmdEnvBlock (opts: InternalOptions): string {
  const prependToPath = cmdEscape(normalizePathEnvVar(opts.prependToPath).win32)
  const nodePath = cmdEscape(normalizePathEnvVar(opts.nodePath).win32)
  let block = ''
  if (prependToPath) {
    block += `@SET "PATH=${prependToPath}:%PATH%"\r\n`
  }
  if (nodePath) {
    block += `\
@IF NOT DEFINED NODE_PATH (\r
  @SET "NODE_PATH=${nodePath}"\r
) ELSE (\r
  @SET "NODE_PATH=${nodePath};%NODE_PATH%"\r
)\r
`
  }
  return block
}

function buildCmdInvocation (opts: {
  args: string
  longProg?: string
  prog: string
  progArgs: string
  target: string
}): string {
  if (opts.longProg) {
    return `\
@IF EXIST ${opts.longProg} (\r
  ${opts.longProg} ${opts.args} ${opts.target} ${opts.progArgs}%*\r
) ELSE (\r
  @SET PATHEXT=%PATHEXT:;.JS;=;%\r
  ${opts.prog} ${opts.args} ${opts.target} ${opts.progArgs}%*\r
)\r
`
  }
  return `@${opts.prog} ${opts.args} ${opts.target} ${opts.progArgs}%*\r\n`
}

function withUtf8Codepage (cmd: string): string {
  if (isAscii(cmd)) return cmd
  const header = '@SETLOCAL\r\n'
  return `${header}\
@SET "_PNPM_CODEPAGE="\r
@FOR /F "tokens=2 delims=:" %%a IN ('"%SystemRoot%\\System32\\chcp.com"') DO @SET "_PNPM_CODEPAGE=%%a"\r
@"%SystemRoot%\\System32\\chcp.com" 65001 >NUL\r
@SET "ERRORLEVEL="\r
${cmd.slice(header.length)}\
@SET "_PNPM_EXIT_CODE=%ERRORLEVEL%"\r
@IF DEFINED _PNPM_CODEPAGE @"%SystemRoot%\\System32\\chcp.com" %_PNPM_CODEPAGE% >NUL\r
@EXIT /B %_PNPM_EXIT_CODE%\r
`
}
