import path from 'node:path'

import type { InternalOptions } from './types.js'
import { isAscii, isWindows, normalizePathEnvVar } from './utils.js'

/**
 * Generate the content of a shim for PowerShell.
 *
 * @param src Path to the executable or script.
 * @param to Path to the shim to be created.
 * It is highly recommended to end with `.ps1`.
 */
export function generatePwshShim (src: string, to: string, opts: InternalOptions): string {
  const shTarget = path.relative(path.dirname(to), src).split('\\').join('/')
  const quotedPathToTarget = path.isAbsolute(shTarget) ? `"${shTarget}"` : `"$basedir/${shTarget}"`
  const { pwshProg, pwshLongProg, target } = resolvePwshPrograms(opts, quotedPathToTarget)
  const args = opts.prog ? (opts.args || '') : ''
  const progArgs = opts.progArgs ? `${opts.progArgs.join(' ')} ` : ''

  let pwsh = buildPwshPreamble(opts)
  pwsh += buildPwshEnvBlocks(opts)
  pwsh += buildPwshExecutionBlock({
    args,
    opts,
    progArgs,
    pwshLongProg,
    pwshProg,
    target,
  })

  return withUtf8Bom(pwsh)
}

function resolvePwshPrograms (opts: InternalOptions, quotedPathToTarget: string): {
  pwshProg: string
  pwshLongProg?: string
  target: string
} {
  const shProg = opts.prog && opts.prog.split('\\').join('/')
  const pwshProg = shProg && `"${shProg}$exe"`

  if (!pwshProg) {
    return {
      pwshProg: quotedPathToTarget,
      target: '',
    }
  }
  if (opts.prog === 'node' && opts.nodeExecPath) {
    return {
      pwshProg: `"${opts.nodeExecPath}"`,
      target: quotedPathToTarget,
    }
  }
  return {
    pwshLongProg: `"$basedir/${opts.prog}$exe"`,
    pwshProg,
    target: quotedPathToTarget,
  }
}

function buildPwshPreamble (opts: InternalOptions): string {
  const normalizedNodePath = normalizePathEnvVar(opts.nodePath)
  const nodePath = normalizedNodePath.win32
  const normalizedPrependPath = normalizePathEnvVar(opts.prependToPath)
  const prependPath = normalizedPrependPath.win32

  return `\
#!/usr/bin/env pwsh
$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent

$exe=""
${(nodePath || prependPath) ? '$pathsep=":"\n' : ''}\
${nodePath ? `\
$env_node_path=$env:NODE_PATH
$new_node_path="${nodePath}"
` : ''}\
${prependPath ? `\
$env_path=$env:PATH
$prepend_path="${prependPath}"
` : ''}\
if ($PSVersionTable.PSVersion -lt "6.0" -or $IsWindows) {
  # Fix case when both the Windows and Linux builds of Node
  # are installed in the same directory
  $exe=".exe"
${(nodePath || prependPath) ? '  $pathsep=";"\n' : ''}\
}`
}

function buildPwshEnvBlocks (opts: InternalOptions): string {
  const normalizedNodePath = normalizePathEnvVar(opts.nodePath)
  const shNodePath = normalizedNodePath.posix
  const normalizedPrependPath = normalizePathEnvVar(opts.prependToPath)
  const shPrependPath = normalizedPrependPath.posix

  let block = ''
  if (shNodePath || shPrependPath) {
    block += `\
 else {
${shNodePath ? `  $new_node_path="${shNodePath}"\n` : ''}\
${shPrependPath ? `  $prepend_path="${shPrependPath}"\n` : ''}\
}
`
  }
  if (shNodePath) {
    block += `\
if ([string]::IsNullOrEmpty($env_node_path)) {
  $env:NODE_PATH=$new_node_path
} else {
  $env:NODE_PATH="$new_node_path$pathsep$env_node_path"
}
`
  }
  if (opts.prependToPath) {
    block += '\n$env:PATH="$prepend_path$pathsep$env:PATH"\n'
  }
  return block
}

function buildPwshExecutionBlock (opts: {
  args: string
  opts: InternalOptions
  progArgs: string
  pwshLongProg?: string
  pwshProg: string
  target: string
}): string {
  const { args, progArgs, pwshLongProg, pwshProg, target } = opts
  const nodePath = normalizePathEnvVar(opts.opts.nodePath).win32
  const prependPath = normalizePathEnvVar(opts.opts.prependToPath).win32
  const restoreEnv = `${nodePath ? '$env:NODE_PATH=$env_node_path\n' : ''}${prependPath ? '$env:PATH=$env_path\n' : ''}`

  if (pwshLongProg) {
    return `\n$ret=0
if (Test-Path ${pwshLongProg}) {
  # Support pipeline input
  if ($MyInvocation.ExpectingInput) {
    $input | & ${pwshLongProg} ${args} ${target} ${progArgs}$args
  } else {
    & ${pwshLongProg} ${args} ${target} ${progArgs}$args
  }
  $ret=$LASTEXITCODE
} else {
  # Support pipeline input
  if ($MyInvocation.ExpectingInput) {
    $input | & ${pwshProg} ${args} ${target} ${progArgs}$args
  } else {
    & ${pwshProg} ${args} ${target} ${progArgs}$args
  }
  $ret=$LASTEXITCODE
}
${restoreEnv}exit $ret
`
  }
  return `\n# Support pipeline input
if ($MyInvocation.ExpectingInput) {
  $input | & ${pwshProg} ${args} ${target} ${progArgs}$args
} else {
  & ${pwshProg} ${args} ${target} ${progArgs}$args
}
${restoreEnv}exit $LASTEXITCODE
`
}

/**
 * Windows PowerShell 5.1 decodes a script without a byte order mark using the
 * ANSI code page, so non-ASCII text in the shim needs a UTF-8 BOM. Elsewhere
 * the shebang has to stay at the start of the file.
 */
function withUtf8Bom (pwsh: string): string {
  if (!isWindows || isAscii(pwsh)) return pwsh
  return `\uFEFF${pwsh}`
}
