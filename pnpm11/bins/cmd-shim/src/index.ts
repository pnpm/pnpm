import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import gfsPromises from '@pnpm/fs.graceful-fs'
import { cmdExtension as CMD_EXTENSION } from 'cmd-extension'

export interface Options {
  /**
   * If a PowerShell script should be created.
   *
   * @default true
   */
  createPwshFile?: boolean

  /**
   * If a Windows Command Prompt script should be created.
   *
   * @default true on Windows, false on other platforms
   */
  createCmdFile?: boolean

  /**
   * If symbolic links should be preserved.
   *
   * @default false
   */
  preserveSymlinks?: boolean

  /**
   * The path to the executable file.
   */
  prog?: string

  /**
   * The arguments to initialize the `node` process with.
   */
  args?: string

  /**
   * The arguments to initialize the target process with, before the actual CLI arguments
   */
  progArgs?: string[]

  /**
   * The value of the $NODE_PATH environment variable.
   *
   * The single `string` format is only kept for legacy compatibility,
   * and the array form should be preferred.
   */
  nodePath?: string | string[]

  /**
   * fs implementation to use.  Must implement node's `fs` module interface.
   */
  fs?: typeof import('fs')

  /*
   * Path to the Node.js executable
   */
  nodeExecPath?: string

  prependToPath?: string

  /** Project or workspace root whose Unix shim paths move together. */
  relocatableRoot?: string
}

/**
 * @internal
 */
type InternalOptions = Options & Required<Pick<Options, keyof typeof DEFAULT_OPTIONS>> & {
  fs_: FsPromises
}

type FsPromises = Pick<typeof fs.promises, 'chmod' | 'mkdir' | 'readFile' | 'stat' | 'unlink' | 'writeFile'>

/**
 * Callback functions to generate scripts for shims.
 * @param src Path to the executable or script.
 * @param to Path to the shim(s) that is going to be created.
 * @param opts Options.
 * @return Generated script for shim.
 */
type ShimGenerator = (src: string, to: string, opts: InternalOptions) => string

interface ShimGenExtTuple {
  generator: ShimGenerator
  extension: string
}

const isWindows = process.platform === 'win32'
const isCygwin = () => isWindows && (process.env.TERM === 'CYGWIN' || process.env.MSYSTEM !== undefined)


// Interpreter paths may contain whitespace other than spaces and tabs.
// eslint-disable-next-line regexp/no-super-linear-backtracking
const shebangExpr = /^#!\s*(?:\/usr\/bin\/env(?:\s+-S)?\s*)?([^ \t]+)(.*)$/
const DEFAULT_OPTIONS = {
  createPwshFile: true,
  createCmdFile: isWindows,
}
/**
 * Map from extensions of files that this module is frequently used for to their runtime.
 * @type {Map<string, string>}
 */
const extensionToProgramMap = new Map([
  ['.js', 'node'],
  ['.cjs', 'node'],
  ['.mjs', 'node'],
  ['.cmd', 'cmd'],
  ['.bat', 'cmd'],
  ['.ps1', 'pwsh'], // not 'powershell'
  ['.sh', 'sh'],
])

function ingestOptions (opts?: Options): InternalOptions {
  const opts_ = {...DEFAULT_OPTIONS, ...opts} as InternalOptions
  opts_.fs_ = opts_.fs ? opts_.fs.promises : (gfsPromises as unknown as FsPromises)
  return opts_
}

/**
 * Try to create shims.
 *
 * @param src Path to program (executable or script).
 * @param to Path to shims.
 * Don't add an extension if you will create multiple types of shims.
 * @param opts Options.
 * @throws If `src` is missing.
 */
export async function cmdShim (src: string, to: string, opts?: Options): Promise<void> {
  const opts_ = ingestOptions(opts)
  await cmdShim_(src, to, opts_)
}

/**
 * Try to create shims.
 *
 * Resolves even when shim creation fails, including when `src` is missing.
 *
 * @param src Path to program (executable or script).
 * @param to Path to shims.
 * Don't add an extension if you will create multiple types of shims.
 * @param opts Options.
 */
export function cmdShimIfExists (src: string, to: string, opts?: Options): Promise<void> {
  return cmdShim(src, to, opts).catch(() => {})
}

/**
 * Check whether a shim's content points at the given source path.
 *
 * @param shimContent The text content of the shim file.
 * @param src The expected source path (the executable the shim should point to).
 * @return `true` if the shim contains a matching target marker.
 */
export function isShimPointingAt (shimContent: string, src: string, opts?: { shimPath: string, relocatableRoot?: string }): boolean {
  const target = opts && isRelocatablePath(src, opts.shimPath, opts.relocatableRoot)
    ? path.relative(path.dirname(opts.shimPath), src)
    : src
  return shimContent.includes(`# ${shimTarget(target)}\n`) &&
    (!opts || !isRelocatablePath(path.dirname(opts.shimPath), opts.shimPath, opts.relocatableRoot) ||
      shimContent.includes(`# ${relocatableRootMarker(opts.shimPath, opts.relocatableRoot!)}\n`))
}

/**
 * Try to unlink, but ignore errors.
 * Any problems will surface later.
 *
 * @param path File to be removed.
 */
function rm (path: string, opts: InternalOptions): Promise<void> {
  return opts.fs_.unlink(path).catch(() => {})
}

async function cmdShim_ (src: string, to: string, opts: InternalOptions) {
  const srcRuntimeInfo = await searchScriptRuntime(src, opts)
  await writeShimsPreCommon(to, opts)
  return writeAllShims(src, to, srcRuntimeInfo, opts)
}

function writeShimsPreCommon (target: string, opts: InternalOptions) {
  return opts.fs_.mkdir(path.dirname(target), { recursive: true })
}

/**
 * Replaces the shell shim and the enabled CMD/PowerShell siblings with executable files.
 * Resolves when all writes and permission changes finish; rejects if any fail.
 */
function writeAllShims (src: string, to: string, srcRuntimeInfo: RuntimeInfo, opts: Options) {
  const opts_ = ingestOptions(opts)
  const generatorAndExts: ShimGenExtTuple[] = [{ generator: generateShShim, extension: '' }]
  if (opts_.createCmdFile) {
    generatorAndExts.push({ generator: generateCmdShim, extension: CMD_EXTENSION })
  }
  if (opts_.createPwshFile) {
    generatorAndExts.push({ generator: generatePwshShim, extension: '.ps1' })
  }
  return Promise.all(
    generatorAndExts.map((generatorAndExt) => writeShim(src, to + generatorAndExt.extension, srcRuntimeInfo, generatorAndExt.generator, opts_))
  )
}

function writeShimPre (target: string, opts: InternalOptions) {
  return rm(target, opts)
}

function writeShimPost (target: string, opts: InternalOptions) {
  return chmodShim(target, opts)
}

interface RuntimeInfo {
  program: string | null
  additionalArgs: string
}

async function searchScriptRuntime (target: string, opts: InternalOptions): Promise<RuntimeInfo> {
  try {
    const data = await opts.fs_.readFile(target, 'utf8')

    // First, check if the bin is a #! of some sort.
    const firstLine = (data as string).trim().split(/\r*\n/)[0]
    const shebang = firstLine.match(shebangExpr)
    if (!shebang) {
      // If not, infer script type from its extension.
      // If the inference fails, it's something that'll be compiled, or some other
      // sort of script, and just call it directly.
      const targetExtension = path.extname(target).toLowerCase()
      // undefined if extension is unknown but it's converted to null.
      const program = extensionToProgramMap.get(targetExtension) || null
      // CMD requires executing batch files with the `/C` flag
      const additionalArgs = program === 'cmd' ? '/C' : ''
      return {
        program,
        additionalArgs,
      }
    }
    return {
      program: shebang[1],
      additionalArgs: shebang[2],
    }
  } catch (err) {
    if (!isWindows || !util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
    if (await opts.fs_.stat(`${target}${getExeExtension()}`)) {
      return {
        program: null,
        additionalArgs: '',
      }
    }
    throw err
  }
}

export function getExeExtension (): string {
  let cmdExtension

  if (process.env.PATHEXT) {
    cmdExtension = process.env.PATHEXT
      .split(path.delimiter)
      .find(ext => ext.toLowerCase() === '.exe')
  }

  return cmdExtension || '.exe'
}

/**
 * Prefix bare `/C` / `/K` switches with an extra `/` so MSYS / Git Bash
 * passes them through to cmd.exe unchanged instead of converting them to
 * `C:\` / `K:\` paths. See {@link generateShShim} for context.
 */
function escapeMsysCmdSwitches (args: string): string {
  return args.replace(/(^|\s)\/([CK])(\s|$)/gi, '$1//$2$3')
}

/**
 * Replaces `to` with an executable shim. Rejects if writing or setting permissions fails.
 */
async function writeShim (src: string, to: string, srcRuntimeInfo: RuntimeInfo, generateShimScript: ShimGenerator, opts: InternalOptions) {
  const defaultArgs = opts.preserveSymlinks ? '--preserve-symlinks' : ''
  const args = [srcRuntimeInfo.additionalArgs, defaultArgs].filter(arg => arg).join(' ')
  opts = Object.assign({}, opts, {
    prog: srcRuntimeInfo.program,
    args: args,
  })

  await writeShimPre(to, opts)
  await opts.fs_.writeFile(to, generateShimScript(src, to, opts), 'utf8')
  return writeShimPost(to, opts)
}

/**
 * Generate the content of a shim for CMD.
 *
 * @param src Path to the executable or script.
 * @param to Path to the shim to be created.
 * It is highly recommended to end with `.cmd` (or `.bat`).
 * @param opts Options.
 * @return The content of shim.
 */
function generateCmdShim (src: string, to: string, opts: InternalOptions): string {
  const shTarget = path.relative(path.dirname(to), src)
  let target = shTarget.split('/').join('\\')
  const quotedPathToTarget = path.isAbsolute(target) ? `"${target}"` : `"%~dp0\\${target}"`
  let longProg
  let prog = opts.prog
  let args = opts.args || ''
  const nodePath = normalizePathEnvVar(opts.nodePath).win32
  const prependToPath = normalizePathEnvVar(opts.prependToPath).win32
  if (!prog) {
    prog = quotedPathToTarget
    args = ''
    target = ''
  } else if (prog === 'node' && opts.nodeExecPath) {
    prog = `"${opts.nodeExecPath}"`
    target = quotedPathToTarget
  } else {
    longProg = `"%~dp0\\${prog}.exe"`
    target = quotedPathToTarget
  }

  let progArgs = opts.progArgs ? `${opts.progArgs.join(' ')} ` : ''

  let cmd = '@SETLOCAL\r\n'
  if (prependToPath) {
    cmd += `@SET "PATH=${prependToPath}:%PATH%"\r\n`
  }
  if (nodePath) {
    cmd += `\
@IF NOT DEFINED NODE_PATH (\r
  @SET "NODE_PATH=${nodePath}"\r
) ELSE (\r
  @SET "NODE_PATH=${nodePath};%NODE_PATH%"\r
)\r
`
  }
  if (longProg) {
    cmd += `\
@IF EXIST ${longProg} (\r
  ${longProg} ${args} ${target} ${progArgs}%*\r
) ELSE (\r
  @SET PATHEXT=%PATHEXT:;.JS;=;%\r
  ${prog} ${args} ${target} ${progArgs}%*\r
)\r
`
  } else {
    cmd += `@${prog} ${args} ${target} ${progArgs}%*\r\n`
  }

  return cmd
}

/**
 * Generate the content of a shim for (Ba)sh in, for example, Cygwin and MSYS(2).
 *
 * @param src Path to the executable or script.
 * @param to Path to the shim to be created.
 * It is highly recommended to end with `.sh` or to contain no extension.
 * @param opts Options.
 * @return The content of shim.
 */
function generateShShim (src: string, to: string, opts: InternalOptions): string {
  const scopedShim = isRelocatablePath(path.dirname(to), to, opts.relocatableRoot)
  const relocatableTarget = isRelocatablePath(src, to, opts.relocatableRoot)
  let shTarget = path.relative(path.dirname(to), src)
  let shProg = opts.prog && opts.prog.split('\\').join('/')
  let shLongProg: string | undefined
  let shLongProgExe = ''
  let shProgExe = ''
  let shProgHasExe = false
  if (!scopedShim) shTarget = shTarget.split('\\').join('/')
  const escapedTarget = scopedShim ? escapeShDoubleQuoted(shTarget) : shTarget
  const quotedPathToTarget = path.isAbsolute(shTarget) ? `"${escapedTarget}"` : `"${relocatableTarget ? '$basedir_abs' : '$basedir'}/${escapedTarget}"`
  const quotedPathToTargetWin = path.isAbsolute(shTarget) ? `"${shTarget}"` : `"$basedir_win/${shTarget}"`
  let shTargetWin = ''
  let args = opts.args || ''
  const isCmdRuntime = opts.prog === 'cmd' || opts.prog === 'cmd.exe'
  const nodePaths = typeof opts.nodePath === 'string' ? opts.nodePath.split(path.delimiter) : opts.nodePath
  const shNodePath = scopedShim
    ? nodePaths?.map(entry => shPath(entry, to, opts.relocatableRoot)).join(':')
    : normalizePathEnvVar(opts.nodePath).posix
  const relocatableNode = opts.nodeExecPath != null && isRelocatablePath(opts.nodeExecPath, to, opts.relocatableRoot)
  const needsAbsoluteBasedir = relocatableTarget || relocatableNode || nodePaths?.some(entry => isRelocatablePath(entry, to, opts.relocatableRoot))
  if (!shProg) {
    shProg = quotedPathToTarget
    args = ''
    shTarget = ''
  } else if (opts.prog === 'node' && opts.nodeExecPath) {
    shProg = `"${shPath(opts.nodeExecPath, to, opts.relocatableRoot)}"`
    shTarget = /\.exe$/.test(opts.nodeExecPath) ? quotedPathToTargetWin : quotedPathToTarget
  } else {
    shProgHasExe = /\.exe$/i.test(shProg)
    shProgExe = shProgHasExe ? shProg : `${shProg}.exe`
    shLongProg = `"$basedir/${shProg}"`
    shLongProgExe = `"$basedir/${shProgExe}"`
    shTarget = quotedPathToTarget
    shTargetWin = quotedPathToTargetWin
  }

  let progArgs = opts.progArgs ? `${opts.progArgs.join(' ')} ` : ''

  let sh = `\
#!/bin/sh
# Resolve $0 through symlinks so basedir is the shim's real directory.
# Cap hops at the kernel's ELOOP limit so a cycle cannot hang the shim.
#
# A shim runs with node_modules/.bin at the front of PATH, so readlink, sed,
# uname, and printf go through \`command -p\`, which searches the system default
# path instead. A dependency's bin cannot stand in for one of them and take over
# the shim before it reaches its target. Directories come from \`\${link%/*}\`,
# which needs no helper at all.
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
  target=$(command -p readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="\${link%/*}/$target" ;;
  esac
done
basedir=$(command -p printf '%s\\n' "$link" | command -p sed -e 's,\\\\,/,g')
basedir="\${basedir%/*}"
basedir_win="$basedir"
exe=""
msys=""

case \`command -p uname -a\` in
  *CYGWIN*|*MINGW*|*MSYS*)
    if command -v cygpath > /dev/null 2>&1; then
      basedir_win=\`cygpath -w "$basedir"\`
    fi
    exe=".exe"
    msys="true"
  ;;
  *WSL2*)
    if command -v wslpath > /dev/null 2>&1; then
      basedir_win="$(wslpath -w "$basedir" 2> /dev/null)"
      if [ $? -ne 0 ] || [ -z "$basedir_win" ]; then
        basedir_win="$basedir"
      else
        exe=".exe"
      fi
    fi
  ;;
esac

`
  if (needsAbsoluteBasedir) {
    sh += 'basedir_abs=$(CDPATH= cd -P -- "$basedir" && pwd -P) || exit $?\n'
  }
  if (opts.prependToPath) {
    sh += `\
export PATH="${opts.prependToPath}:$PATH"
`
  }
  if (shNodePath) {
    sh += `\
if [ -z "$NODE_PATH" ]; then
  export NODE_PATH="${shNodePath}"
else
  export NODE_PATH="${shNodePath}:$NODE_PATH"
fi
`
  }

  const generateExecBlock = (execArgs: string) => {
    if (shLongProg) {
      if (shProgHasExe) {
        return `\
if [ -x ${shLongProgExe} ]; then
  exec ${shLongProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
else
  exec ${shProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
fi
`
      } else {
        return `\
if [ -n "$exe" ] && [ -x ${shLongProgExe} ]; then
  exec ${shLongProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
elif [ -x ${shLongProg} ]; then
  exec ${shLongProg} ${execArgs} ${shTarget} ${progArgs}"$@"
elif command -v ${shProg} >/dev/null 2>&1; then
  exec ${shProg} ${execArgs} ${shTarget} ${progArgs}"$@"
elif [ -n "$exe" ] && command -v ${shProgExe} >/dev/null 2>&1; then
  exec ${shProgExe} ${execArgs} ${shTargetWin} ${progArgs}"$@"
else
  exec ${shProg} ${execArgs} ${shTarget} ${progArgs}"$@"
fi
`
      }
    } else {
      return `\
exec ${shProg} ${execArgs} ${shTarget} ${progArgs}"$@"
exit $?
`
    }
  }

  const msysArgs = isCmdRuntime ? escapeMsysCmdSwitches(args) : args
  if (msysArgs !== args) {
    sh += `\
if [ -n "$msys" ]; then
${indentShellBlock(generateExecBlock(msysArgs))}
else
${indentShellBlock(generateExecBlock(args))}
fi
`
  } else {
    sh += generateExecBlock(args)
  }

  // Marker used by consumers to detect whether the shim is up-to-date
  // without parsing the script content.
  sh += `# ${shimTarget(relocatableTarget ? path.relative(path.dirname(to), src) : src)}\n`
  if (isRelocatablePath(path.dirname(to), to, opts.relocatableRoot)) {
    sh += `# ${relocatableRootMarker(to, opts.relocatableRoot!)}\n`
  }

  return sh
}

function indentShellBlock (script: string): string {
  return script.split('\n').map(line => line ? `  ${line}` : line).join('\n')
}

/**
 * Generate the content of a shim for PowerShell.
 *
 * @param src Path to the executable or script.
 * @param to Path to the shim to be created.
 * It is highly recommended to end with `.ps1`.
 * @param opts Options.
 * @return The content of shim.
 */
function generatePwshShim (src: string, to: string, opts: InternalOptions): string {
  let shTarget = path.relative(path.dirname(to), src)
  const shProg = opts.prog && opts.prog.split('\\').join('/')
  let pwshProg = shProg && `"${shProg}$exe"`
  let pwshLongProg
  shTarget = shTarget.split('\\').join('/')
  const quotedPathToTarget = path.isAbsolute(shTarget) ? `"${shTarget}"` : `"$basedir/${shTarget}"`
  let args = opts.args || ''
  let normalizedNodePathEnvVar = normalizePathEnvVar(opts.nodePath)
  const nodePath = normalizedNodePathEnvVar.win32
  const shNodePath = normalizedNodePathEnvVar.posix
  let normalizedPrependPathEnvVar = normalizePathEnvVar(opts.prependToPath)
  const prependPath = normalizedPrependPathEnvVar.win32
  const shPrependPath = normalizedPrependPathEnvVar.posix
  if (!pwshProg) {
    pwshProg = quotedPathToTarget
    args = ''
    shTarget = ''
  } else if (opts.prog === 'node' && opts.nodeExecPath) {
    pwshProg = `"${opts.nodeExecPath}"`
    shTarget = quotedPathToTarget
  } else {
    pwshLongProg = `"$basedir/${opts.prog}$exe"`
    shTarget = quotedPathToTarget
  }

  let progArgs = opts.progArgs ? `${opts.progArgs.join(' ')} ` : ''

  let pwsh = `\
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
  if (shNodePath || shPrependPath) {
    pwsh += `\
 else {
${shNodePath ? `  $new_node_path="${shNodePath}"\n` : ''}\
${shPrependPath ? `  $prepend_path="${shPrependPath}"\n` : ''}\
}
`
  }
  if (shNodePath) {
    pwsh += `\
if ([string]::IsNullOrEmpty($env_node_path)) {
  $env:NODE_PATH=$new_node_path
} else {
  $env:NODE_PATH="$new_node_path$pathsep$env_node_path"
}
`
  }
  if (opts.prependToPath) {
    pwsh += `
$env:PATH="$prepend_path$pathsep$env:PATH"
`
  }
  if (pwshLongProg) {
    pwsh += `
$ret=0
if (Test-Path ${pwshLongProg}) {
  # Support pipeline input
  if ($MyInvocation.ExpectingInput) {
    $input | & ${pwshLongProg} ${args} ${shTarget} ${progArgs}$args
  } else {
    & ${pwshLongProg} ${args} ${shTarget} ${progArgs}$args
  }
  $ret=$LASTEXITCODE
} else {
  # Support pipeline input
  if ($MyInvocation.ExpectingInput) {
    $input | & ${pwshProg} ${args} ${shTarget} ${progArgs}$args
  } else {
    & ${pwshProg} ${args} ${shTarget} ${progArgs}$args
  }
  $ret=$LASTEXITCODE
}
${nodePath ? '$env:NODE_PATH=$env_node_path\n' : ''}\
${prependPath ? '$env:PATH=$env_path\n' : ''}\
exit $ret
`
  } else {
    pwsh += `
# Support pipeline input
if ($MyInvocation.ExpectingInput) {
  $input | & ${pwshProg} ${args} ${shTarget} ${progArgs}$args
} else {
  & ${pwshProg} ${args} ${shTarget} ${progArgs}$args
}
${nodePath ? '$env:NODE_PATH=$env_node_path\n' : ''}\
${prependPath ? '$env:PATH=$env_path\n' : ''}\
exit $LASTEXITCODE
`
  }

  return pwsh
}

function chmodShim (to: string, opts: InternalOptions) {
  return opts.fs_.chmod(to, 0o755)
}

interface NormalizedPathEnvVar {
  win32: string
  posix: string
  [index: number]: {win32: string, posix: string}
}
function normalizePathEnvVar (nodePath: undefined | string | string[]): NormalizedPathEnvVar {
  if (!nodePath || !nodePath.length) {
    return {
      win32: '',
      posix: '',
    }
  }
  let split = (typeof nodePath === 'string' ? nodePath.split(path.delimiter) : Array.from(nodePath))
  let result = {} as NormalizedPathEnvVar
  for (let i = 0; i < split.length; i++) {
    const win32 = split[i].split('/').join('\\')
    const posix = isWindows ? split[i].split('\\').join('/').replace(/^([^:\\/]*):/, (_, $1) => `${isCygwin() ? '/proc/cygdrive' : '/mnt'}/${$1.toLowerCase()}`) : split[i]

    result.win32 = result.win32 ? `${result.win32};${win32}` : win32
    result.posix = result.posix ? `${result.posix}:${posix}` : posix

    result[i] = {win32, posix}
  }
  return result
}

function shimTarget (src: string): string {
  return `cmd-shim-target=${src.split('\\').join('/')}`
}

function isRelocatablePath (target: string, shimPath: string, root?: string): boolean {
  return !isWindows && root != null && isWithinRoot(root, path.dirname(shimPath)) && isWithinRoot(root, target)
}

function isWithinRoot (root: string, target: string): boolean {
  const relative = path.relative(root, target)
  return relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)
}

function shPath (target: string, shimPath: string, root?: string): string {
  if (isRelocatablePath(target, shimPath, root)) {
    return `$basedir_abs/${escapeShDoubleQuoted(path.relative(path.dirname(shimPath), target))}`
  }
  return isRelocatablePath(path.dirname(shimPath), shimPath, root) ? escapeShDoubleQuoted(target) : target
}

function escapeShDoubleQuoted (text: string): string {
  return Array.from(text, character => ['\\', '"', '$', '`'].includes(character) ? `\\${character}` : character).join('')
}

function relocatableRootMarker (shimPath: string, root: string): string {
  return `cmd-shim-relocatable-root=${path.relative(path.dirname(shimPath), root)}`
}
