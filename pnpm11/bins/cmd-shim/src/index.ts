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

  /**
   * The directory the shell shim computes its relative target from, as
   * {@link getShShimDir} returns it. Computed when omitted.
   */
  shShimDir?: string
}

export interface GetShShimDirOptions {
  /** The shim directory with its symlinks resolved, when the caller already has it. */
  physicalDir?: string
  realpath?: (path: string) => Promise<string>
}

/**
 * @internal
 */
type InternalOptions = Options & Required<Pick<Options, keyof typeof DEFAULT_OPTIONS>> & {
  fs_: FsPromises
  isTargetMissing?: boolean
  /** Reject a missing source instead of shimming it. */
  requireSource?: boolean
}

type FsPromises = Pick<typeof fs.promises, 'chmod' | 'mkdir' | 'readFile' | 'realpath' | 'stat' | 'unlink' | 'writeFile'>

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
  opts_.fs_ = opts_.fs ? opts_.fs.promises : ({ ...gfsPromises, realpath: fs.promises.realpath } as unknown as FsPromises)
  return opts_
}

/**
 * Try to create shims.
 *
 * A missing `src` gets a shim whose runtime is inferred from its extension.
 *
 * @param src Path to program (executable or script).
 * @param to Path to shims.
 * Don't add an extension if you will create multiple types of shims.
 * @param opts Options.
 * @throws On any other failure to read `src` or to write the shims.
 */
export async function cmdShim (src: string, to: string, opts?: Options): Promise<void> {
  const opts_ = ingestOptions(opts)
  await cmdShim_(src, to, opts_)
}

/**
 * Try to create shims.
 *
 * Does nothing when `src` is missing (on Windows, when `src.exe` is missing
 * too), and resolves even when shim creation fails.
 *
 * @param src Path to program (executable or script).
 * @param to Path to shims.
 * Don't add an extension if you will create multiple types of shims.
 * @param opts Options.
 */
export async function cmdShimIfExists (src: string, to: string, opts?: Options): Promise<void> {
  try {
    await cmdShim_(src, to, { ...ingestOptions(opts), requireSource: true })
  } catch {}
}

/**
 * Check whether a shim's content points at the given source path.
 *
 * @param shimContent The text content of the shim file.
 * @param src The expected source path (the executable the shim should point to).
 * @return `true` if the shim contains a matching target marker.
 */
export function isShimPointingAt (shimContent: string, src: string): boolean {
  return shimContent.includes(`# ${shimTarget(src)}\n`)
}

/**
 * Whether the shell shim `shimContent`, as written by {@link cmdShim}, was
 * written while its target was missing. Its runtime was then inferred from the
 * target's extension, so it should be rewritten once the target exists and its
 * shebang can be read. Content without the marker line, including a shim from
 * an older cmd-shim, counts as written for an existing target.
 */
export function isShimForMissingTarget (shimContent: string): boolean {
  return shimContent.includes(`${TARGET_MISSING_MARKER}\n`)
}

const TARGET_MISSING_MARKER = '# cmd-shim-missing-target'

/**
 * Check whether a shell shim's `NODE_PATH` starts with `first` and ends with
 * the `last` entries. An omitted `first` or `last` is not checked. When both
 * are empty, the shim must not set `NODE_PATH` at all.
 */
export function isShimNodePath (shimContent: string, expected: { first?: string, last?: string[] }): boolean {
  const first = expected.first == null ? '' : normalizePathEnvVar([expected.first]).posix
  const last = normalizePathEnvVar(expected.last).posix
  const value = readShNodePath(shimContent)
  if (value == null) return first === '' && last === ''
  return (first !== '' || last !== '') &&
    (first === '' || value === first || value.startsWith(`${first}:`)) &&
    (last === '' || value === last || value.endsWith(`:${last}`))
}

/**
 * The posix form of the shim's own `NODE_PATH` entries. A shim written on
 * Windows assigns it to `new_node_path` and picks a form when it runs, so a
 * Windows shim that exports it directly came from an older version and reads
 * as empty. Any other shim exports it in the branch that runs when the caller
 * has no `NODE_PATH` of its own.
 */
export function readShNodePath (shimContent: string): string | undefined {
  const winStart = shimContent.indexOf('\nelse\n  new_node_path=')
  if (winStart !== -1) {
    const valueStart = winStart + '\nelse\n  new_node_path='.length
    const lineEnd = shimContent.indexOf('\n', valueStart)
    const raw = lineEnd === -1 ? shimContent.slice(valueStart) : shimContent.slice(valueStart, lineEnd)
    return unquoteSh(raw)
  }
  if (isWindows) {
    return shimContent.includes(SH_NODE_PATH_EXPORT) ? '' : undefined
  }
  const start = shimContent.indexOf(SH_NODE_PATH_EXPORT)
  if (start === -1) return undefined
  const valueStart = start + SH_NODE_PATH_EXPORT.length
  const lineEnd = shimContent.indexOf('\n', valueStart)
  const raw = lineEnd === -1 ? shimContent.slice(valueStart) : shimContent.slice(valueStart, lineEnd)
  return unquoteSh(raw)
}

function unquoteSh (raw: string): string {
  const trimmed = raw.trim()
  if (trimmed.startsWith("'") && trimmed.endsWith("'")) {
    return trimmed.slice(1, -1).replaceAll("'\\''", "'")
  }
  if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
    return trimmed.slice(1, -1).replaceAll('\\"', '"').replaceAll('\\\\', '\\')
  }
  return trimmed
}

const SH_NODE_PATH_EXPORT = '\n  export NODE_PATH='

/**
 * Node normalizes the `..` segments of a relative target lexically, so they
 * climb from the shim's physical directory. It is set before the Windows-form
 * `$basedir_win` is derived, so the paths handed to a Windows runtime are
 * physical too.
 */
const SH_SHIM_BASEDIR_ABS_PRELUDE = `\
basedir_abs=$(CDPATH= cd -P -- "$basedir" && pwd -P) || exit $?
basedir="$basedir_abs"
`

/**
 * Whether a shell shim anchors its target on its physical directory exactly
 * when its target, relative to the shim's directory, needs it. Keep this in
 * step with pacquet's `is_sh_shim_basedir_anchor_current`.
 */
export function isShimBasedirAnchorCurrent (shimContent: string, relativeTarget: string): boolean {
  return shimContent.includes(SH_SHIM_BASEDIR_ABS_PRELUDE) !== path.isAbsolute(relativeTarget)
}

/**
 * The POSIX relative target path stored in `shimContent`. Returns `undefined`
 * when the shim does not anchor on a relative target.
 */
export function readShRelativeTarget (shimContent: string): string | undefined {
  const marker = '"$basedir_abs/'
  const start = shimContent.indexOf(marker)
  if (start === -1) return undefined
  const valueStart = start + marker.length
  const end = shimContent.indexOf('"', valueStart)
  if (end === -1) return undefined
  return shimContent.slice(valueStart, end)
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
  const shShimDir = opts.shShimDir ?? await getShShimDir(src, to, { realpath: async (dir) => opts.fs_.realpath(dir) })
  return writeAllShims(src, to, srcRuntimeInfo, { ...opts, shShimDir })
}

/**
 * The shell shim climbs to a relative target from its physical directory, so
 * the relative target is computed from there too. That is the shim's own
 * directory unless a symlink lies on the way. Then it is the physical
 * directory, placed under the lexical ancestor it shares with `src` when it
 * lies under that ancestor's physical path, which also keeps a `subst` drive
 * on Windows.
 */
export async function getShShimDir (src: string, to: string, opts: GetShShimDirOptions = {}): Promise<string> {
  const dir = path.dirname(to)
  const realpath = opts.realpath ?? fs.promises.realpath
  const physicalDir = opts.physicalDir ?? await getPhysicalShimDir(dir, realpath)
  if (physicalDir === dir) return dir
  let ancestor = dir
  while (!isSubdirOrEqual(ancestor, src)) {
    const parent = path.dirname(ancestor)
    if (parent === ancestor) return physicalDir
    ancestor = parent
  }
  const physicalAncestor = await realpath(ancestor)
  return isSubdirOrEqual(physicalAncestor, physicalDir)
    ? path.join(ancestor, path.relative(physicalAncestor, physicalDir))
    : physicalDir
}

/**
 * `dir` with its symlinks resolved, as the shell shim's `cd -P` resolves them.
 * On Windows `dir` is resolved only when a symlink or junction lies on its
 * path, since `realpath` also resolves a `subst` drive, which the MSYS shell
 * does not.
 */
export async function getPhysicalShimDir (dir: string, realpath: (path: string) => Promise<string> = fs.promises.realpath): Promise<string> {
  if (isWindows && !await hasLinkOnPath(dir)) return dir
  return realpath(dir)
}

async function hasLinkOnPath (dir: string): Promise<boolean> {
  const ancestors = [dir]
  for (let parent = path.dirname(dir); parent !== ancestors.at(-1); parent = path.dirname(parent)) {
    ancestors.push(parent)
  }
  const isLink = await Promise.all(ancestors.map(isSymbolicLink))
  return isLink.includes(true)
}

async function isSymbolicLink (file: string): Promise<boolean> {
  try {
    return (await fs.promises.lstat(file)).isSymbolicLink()
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

function isSubdirOrEqual (parent: string, child: string): boolean {
  const relative = path.relative(parent, child)
  return relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)
}

function writeShimsPreCommon (target: string, opts: InternalOptions) {
  return opts.fs_.mkdir(path.dirname(target), { recursive: true })
}

/**
 * Replaces the shell shim and the enabled CMD/PowerShell siblings with executable files.
 * Resolves when all writes and permission changes finish; rejects if any fail.
 */
function writeAllShims (src: string, to: string, srcRuntimeInfo: RuntimeInfo, opts: InternalOptions) {
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
  /** Whether `program` was inferred from the path because the target is missing. */
  isTargetMissing?: boolean
}

async function searchScriptRuntime (target: string, opts: InternalOptions): Promise<RuntimeInfo> {
  let data: string
  try {
    data = await opts.fs_.readFile(target, 'utf8') as string
  } catch (err) {
    if (!isMissingPathError(err)) throw err
    // The target may be created after linking, for instance by a build step,
    // so the shim is written with the runtime inferred from the path alone.
    if (isWindows && path.extname(target) === '' && await exists(`${target}${getExeExtension()}`, opts)) {
      return {
        program: null,
        additionalArgs: '',
      }
    }
    if (opts.requireSource) throw err
    return { ...runtimeFromExtension(target), isTargetMissing: true }
  }

  // First, check if the bin is a #! of some sort.
  const firstLine = data.trim().split(/\r*\n/)[0]
  const shebang = firstLine.match(shebangExpr)
  if (!shebang) {
    return runtimeFromExtension(target)
  }
  return {
    program: shebang[1],
    additionalArgs: shebang[2],
  }
}

/**
 * Infer the script type from the target's extension. If the inference fails,
 * it's something that'll be compiled, or some other sort of script, and is
 * called directly.
 */
function runtimeFromExtension (target: string): RuntimeInfo {
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

async function exists (file: string, opts: InternalOptions): Promise<boolean> {
  try {
    await opts.fs_.stat(file)
    return true
  } catch (err) {
    if (isMissingPathError(err)) return false
    throw err
  }
}

function isMissingPathError (err: unknown): boolean {
  return util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')
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
    isTargetMissing: srcRuntimeInfo.isTargetMissing,
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
  let target = cmdEscape(shTarget.split('/').join('\\'))
  const quotedPathToTarget = path.isAbsolute(target) ? `"${target}"` : `"%~dp0\\${target}"`
  let longProg
  let prog = opts.prog
  let args = cmdEscape(opts.args || '')
  const nodePath = cmdEscape(normalizePathEnvVar(opts.nodePath).win32)
  const prependToPath = cmdEscape(normalizePathEnvVar(opts.prependToPath).win32)
  if (!prog) {
    prog = quotedPathToTarget
    args = ''
    target = ''
  } else if (prog === 'node' && opts.nodeExecPath) {
    prog = `"${cmdEscape(opts.nodeExecPath)}"`
    target = quotedPathToTarget
  } else {
    prog = cmdEscape(prog)
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
  let shTarget = path.relative(opts.shShimDir ?? path.dirname(to), src)
  let shProg = opts.prog && opts.prog.split('\\').join('/')
  let shLongProg: string | undefined
  let shLongProgExe = ''
  let shProgExe = ''
  let shProgHasExe = false
  shTarget = shTarget.split('\\').join('/')
  const isTargetAbsolute = path.isAbsolute(shTarget)
  const quotedPathToTarget = isTargetAbsolute ? `"${shTarget}"` : `"$basedir_abs/${shTarget}"`
  const quotedPathToTargetWin = isTargetAbsolute ? `"${shTarget}"` : `"$basedir_win/${shTarget}"`
  let shTargetWin = ''
  let args = opts.args || ''
  const isCmdRuntime = opts.prog === 'cmd' || opts.prog === 'cmd.exe'
  const shNodePath = normalizePathEnvVar(opts.nodePath).posix
  if (!shProg) {
    shProg = quotedPathToTarget
    args = ''
    shTarget = ''
  } else if (opts.prog === 'node' && opts.nodeExecPath) {
    shProg = `"${opts.nodeExecPath}"`
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
#
# Where no default path is compiled in, as on Nix, \`command -p\` searches PATH
# instead, so the helpers run with node_modules and relative entries dropped from
# PATH.
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
${isTargetAbsolute ? '' : SH_SHIM_BASEDIR_ABS_PRELUDE}basedir_win="$basedir"
exe=""
msys=""

case \`command -p uname -a\` in
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
  if (opts.prependToPath) {
    sh += `\
export PATH="${opts.prependToPath}:$PATH"
`
  }
  if (shNodePath && isWindows) {
    // Cygwin and MSYS start the native Windows node, which reads the win32
    // form; MSYS would move a /mnt/c path under its own install directory.
    // WSL reads the /mnt form. The shim picks one when it runs, so the result
    // doesn't depend on the shell that installed it.
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
        // On Cygwin and MSYS, the program on PATH is usually a native Windows
        // one, such as node.exe. Cygwin doesn't convert POSIX path arguments
        // for it, so it gets the win32 form of the target.
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
  if (opts.isTargetMissing) sh += `${TARGET_MISSING_MARKER}\n`
  sh += `# ${shimTarget(src)}\n`

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
    const posix = isWindows ? split[i].split('\\').join('/').replace(/^([^:\\/]*):/, (_, $1) => `/mnt/${$1.toLowerCase()}`) : split[i]

    result.win32 = result.win32 ? `${result.win32};${win32}` : win32
    result.posix = result.posix ? `${result.posix}:${posix}` : posix

    result[i] = {win32, posix}
  }
  return result
}

/**
 * Escape `text` for a `.cmd` file, where `%` would otherwise expand as a
 * variable reference, even inside double quotes.
 */
function cmdEscape (text: string): string {
  return text.replaceAll('%', '%%')
}

function shSingleQuote (text: string): string {
  return `'${text.replaceAll("'", "'\\''")}'`
}

function shimTarget (src: string): string {
  return `cmd-shim-target=${src.split('\\').join('/')}`
}
