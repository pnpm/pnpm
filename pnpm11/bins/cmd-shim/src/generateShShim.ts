import path from 'node:path'

import type { InternalOptions } from './types.js'
import { isWindows, normalizePathEnvVar, shimTarget, shSingleQuote } from './utils.js'

export const TARGET_MISSING_MARKER = '# cmd-shim-missing-target'
export const SH_NODE_PATH_EXPORT = '\n  export NODE_PATH='
export const SH_SHIM_BASEDIR_ABS_PRELUDE = `\
basedir_abs=$(CDPATH= cd -P -- "$basedir" && pwd -P) || exit $?
basedir="$basedir_abs"
`

interface ShProgConfig {
  args: string
  isCmdRuntime: boolean
  isTargetAbsolute: boolean
  progArgs: string
  shLongProg?: string
  shLongProgExe: string
  shProg: string
  shProgExe: string
  shProgHasExe: boolean
  shTarget: string
  shTargetWin: string
}

/**
 * Generate the content of a shim for (Ba)sh in, for example, Cygwin and MSYS(2).
 *
 * @param src Path to the executable or script.
 * @param to Path to the shim to be created.
 * It is highly recommended to end with `.sh` or to contain no extension.
 */
export function generateShShim (src: string, to: string, opts: InternalOptions): string {
  const config = resolveShProgConfig(src, to, opts)

  let sh = buildShHeader(config.isTargetAbsolute)
  sh += buildShPathAndNodePath(opts)
  sh += buildShExecBlock(config)

  if (opts.isTargetMissing) sh += `${TARGET_MISSING_MARKER}\n`
  sh += `# ${shimTarget(src)}\n`

  return sh
}

function resolveShProgConfig (src: string, to: string, opts: InternalOptions): ShProgConfig {
  const rawTarget = path.relative(opts.shShimDir ?? path.dirname(to), src).split('\\').join('/')
  const isTargetAbsolute = path.isAbsolute(rawTarget)
  const quotedTarget = isTargetAbsolute ? `"${rawTarget}"` : `"$basedir_abs/${rawTarget}"`
  const quotedTargetWin = isTargetAbsolute ? `"${rawTarget}"` : `"$basedir_win/${rawTarget}"`
  const progArgs = opts.progArgs ? `${opts.progArgs.join(' ')} ` : ''
  const isCmdRuntime = opts.prog === 'cmd' || opts.prog === 'cmd.exe'

  if (!opts.prog) {
    return {
      args: '',
      isCmdRuntime,
      isTargetAbsolute,
      progArgs,
      shLongProgExe: '',
      shProg: quotedTarget,
      shProgExe: '',
      shProgHasExe: false,
      shTarget: '',
      shTargetWin: '',
    }
  }

  return resolveShProgramTarget({
    isCmdRuntime,
    isTargetAbsolute,
    nodeExecPath: opts.nodeExecPath,
    prog: opts.prog,
    progArgs,
    quotedTarget,
    quotedTargetWin,
    rawArgs: opts.args || '',
  })
}

function resolveShProgramTarget (opts: {
  isCmdRuntime: boolean
  isTargetAbsolute: boolean
  nodeExecPath?: string
  prog: string
  progArgs: string
  quotedTarget: string
  quotedTargetWin: string
  rawArgs: string
}): ShProgConfig {
  const { isCmdRuntime, isTargetAbsolute, nodeExecPath, prog, progArgs, quotedTarget, quotedTargetWin, rawArgs } = opts
  const shProg = prog.split('\\').join('/')
  if (prog === 'node' && nodeExecPath) {
    return {
      args: rawArgs,
      isCmdRuntime,
      isTargetAbsolute,
      progArgs,
      shLongProgExe: '',
      shProg: `"${nodeExecPath}"`,
      shProgExe: '',
      shProgHasExe: false,
      shTarget: /\.exe$/.test(nodeExecPath) ? quotedTargetWin : quotedTarget,
      shTargetWin: '',
    }
  }
  const shProgHasExe = /\.exe$/i.test(shProg)
  const shProgExe = shProgHasExe ? shProg : `${shProg}.exe`
  return {
    args: rawArgs,
    isCmdRuntime,
    isTargetAbsolute,
    progArgs,
    shLongProg: `"$basedir/${shProg}"`,
    shLongProgExe: `"$basedir/${shProgExe}"`,
    shProg,
    shProgExe,
    shProgHasExe,
    shTarget: quotedTarget,
    shTargetWin: quotedTargetWin,
  }
}

function buildShHeader (isTargetAbsolute: boolean): string {
  const basedirAbsPrelude = isTargetAbsolute ? '' : SH_SHIM_BASEDIR_ABS_PRELUDE
  return `\
${SH_PRELUDE_HEADER}\
${SH_RESOLVE_LINK_LOOP}\
${basedirAbsPrelude}\
${SH_WINDOWS_SUBSYSTEM_DETECT}\
`
}

const SH_PRELUDE_HEADER = `\
#!/bin/sh
# Resolve $0 through symlinks so basedir is the shim's real directory.
# Cap hops at the kernel's ELOOP limit so a cycle cannot hang the shim.
#
# A shim runs with node_modules/.bin at the front of PATH, so readlink, sed,
# uname, and printf go through \`command -p\`, which searches the system default
# path instead. A dependency's bin cannot stand in for one of them and take over
# the shim before it reaches its target. Directories come from \`\${link%/*}\`,
# which needs no helper at all.
#
# A helper the default path lacks, as in a Nix build sandbox, comes from PATH
# instead, with node_modules and relative entries dropped from it.
caller_path_set=\${PATH+set}
caller_path=\${PATH-}
helper_path=
rest=$caller_path:
while [ -n "$rest" ]; do
  dir=\${rest%%:*}
  rest=\${rest#*:}
  case "$dir" in
    */node_modules/*|*/node_modules) ;;
    /*) helper_path=\${helper_path:+$helper_path:}$dir ;;
  esac
done
# An empty PATH searches the current directory.
PATH=\${helper_path:-/}
# A helper comes from PATH only when the default path lacks it and PATH has it.
# The bash 3.2 that macOS ships as sh answers \`command -p -v\` from PATH.
run_helper() {
  if command -p -v "$1" >/dev/null 2>&1 || ! command -v "$1" >/dev/null 2>&1; then command -p "$@"; else command "$@"; fi
}
`

const SH_RESOLVE_LINK_LOOP = `\
link="$0"
# \`\${link%/*}\` needs a separator to strip. A bare name came from a PATH lookup
# and stands for a file in the current directory.
case "$link" in
  */*|*\\\\*) ;;
  *) link="./$link" ;;
esac
hops=0
while [ -L "$link" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops+1))
  target=$(run_helper readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="\${link%/*}/$target" ;;
  esac
done
basedir=$(run_helper printf '%s\\n' "$link" | run_helper sed -e 's,\\\\,/,g')
basedir="\${basedir%/*}"
`

const SH_WINDOWS_SUBSYSTEM_DETECT = `\
basedir_win="$basedir"
exe=""
msys=""

case \`run_helper uname -a\` in
  *CYGWIN*|*MINGW*|*MSYS*)
    if converted=$(command -p cygpath -w "$basedir" 2>/dev/null) && [ -n "$converted" ]; then
      basedir_win="$converted"
    elif command -v cygpath > /dev/null 2>&1; then
      basedir_win=\`cygpath -w "$basedir"\`
    fi
    exe=".exe"
    msys="true"
  ;;
  *WSL2*)
    if converted=$(command -p wslpath -w "$basedir" 2>/dev/null) && [ -n "$converted" ]; then
      basedir_win="$converted"
      exe=".exe"
    elif command -v wslpath > /dev/null 2>&1; then
      basedir_win="$(wslpath -w "$basedir" 2> /dev/null)"
      if [ $? -ne 0 ] || [ -z "$basedir_win" ]; then
        basedir_win="$basedir"
      else
        exe=".exe"
      fi
    fi
  ;;
esac
if [ -n "$caller_path_set" ]; then PATH=$caller_path; else unset PATH; fi

`

function buildShPathAndNodePath (opts: InternalOptions): string {
  let sh = ''
  if (opts.prependToPath) {
    sh += `export PATH="${opts.prependToPath}:$PATH"\n`
  }
  const shNodePath = normalizePathEnvVar(opts.nodePath).posix
  if (shNodePath && isWindows) {
    sh += `\
if [ -n "$msys" ]; then
  new_node_path=${shSingleQuote(normalizePathEnvVar(opts.nodePath).win32)}
  node_path_sep=';'
else
  new_node_path=${shSingleQuote(shNodePath)}
  node_path_sep=':'
fi
if [ -z "$NODE_PATH" ]; then
  export NODE_PATH="$new_node_path"
else
  export NODE_PATH="$new_node_path$node_path_sep$NODE_PATH"
fi
`
  } else if (shNodePath) {
    sh += `\
if [ -z "$NODE_PATH" ]; then
  export NODE_PATH="${shNodePath}"
else
  export NODE_PATH="${shNodePath}:$NODE_PATH"
fi
`
  }
  return sh
}

function buildShExecBlock (config: ShProgConfig): string {
  const msysArgs = config.isCmdRuntime ? escapeMsysCmdSwitches(config.args) : config.args
  if (msysArgs !== config.args) {
    return `\
if [ -n "$msys" ]; then
${indentShellBlock(generateExecBlock(config, msysArgs))}
else
${indentShellBlock(generateExecBlock(config, config.args))}
fi
`
  }
  return generateExecBlock(config, config.args)
}

function generateExecBlock (config: ShProgConfig, execArgs: string): string {
  const { shLongProg, shLongProgExe, shProg, shProgExe, shProgHasExe, shTarget, shTargetWin, progArgs } = config
  if (!shLongProg) {
    return `\
exec ${shProg} ${execArgs} ${shTarget} ${progArgs}"$@"
exit $?
`
  }
  if (shProgHasExe) {
    return `\
if [ -x ${shLongProgExe} ]; then
  exec ${shLongProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
else
  exec ${shProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
fi
`
  }
  return `\
if [ -n "$exe" ] && [ -x ${shLongProgExe} ]; then
  exec ${shLongProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
elif [ -x ${shLongProg} ]; then
  exec ${shLongProg} ${execArgs} ${shTarget} ${progArgs}"$@"
elif [ -n "$msys" ] && command -v ${shProg} >/dev/null 2>&1; then
  exec ${shProg} ${execArgs} ${shTargetWin} ${progArgs}"$@"
elif command -v ${shProg} >/dev/null 2>&1; then
  exec ${shProg} ${execArgs} ${shTarget} ${progArgs}"$@"
elif [ -n "$exe" ] && command -v ${shProgExe} >/dev/null 2>&1; then
  exec ${shProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
else
  exec ${shProg} ${execArgs} ${shTarget} ${progArgs}"$@"
fi
`
}

function escapeMsysCmdSwitches (args: string): string {
  return args.replace(/(^|\s)\/([CK])(\s|$)/gi, '$1//$2$3')
}

function indentShellBlock (script: string): string {
  return script.split('\n').map((line) => line ? `  ${line}` : line).join('\n')
}
