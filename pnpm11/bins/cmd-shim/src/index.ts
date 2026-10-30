import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import gfsPromises from '@pnpm/fs.graceful-fs'
import { cmdExtension as CMD_EXTENSION } from 'cmd-extension'

import { generateCmdShim } from './generateCmdShim.js'
import { generatePwshShim } from './generatePwshShim.js'
import {
  generateShShim,
  isShimMsysCmdSwitchDoubled,
  SH_NODE_PATH_EXPORT,
  SH_SHIM_BASEDIR_ABS_PRELUDE,
  TARGET_MISSING_MARKER,
} from './generateShShim.js'
import type {
  FsPromises,
  GetShShimDirOptions,
  InternalOptions,
  Options,
  RuntimeInfo,
  ShimDirFs,
  ShimGenerator,
  ShimGenExtTuple,
} from './types.js'
import {
  DEFAULT_OPTIONS,
} from './types.js'
import {
  isWindows,
  normalizePathEnvVar,
  shimTarget,
} from './utils.js'

export type { GetShShimDirOptions, Options, ShimDirFs }

// Interpreter paths may contain whitespace other than spaces and tabs.
// eslint-disable-next-line regexp/no-super-linear-backtracking -- only matched against the first line of a script, after the `#!` anchor
const shebangExpr = /^#!\s*(?:\/usr\/bin\/env(?:\s+-S)?\s*)?([^ \t]+)(.*)$/

const extensionToProgramMap = new Map([
  ['.js', 'node'],
  ['.cjs', 'node'],
  ['.mjs', 'node'],
  ['.cmd', 'cmd'],
  ['.bat', 'cmd'],
  ['.ps1', 'pwsh'],
  ['.sh', 'sh'],
])

function ingestOptions (opts?: Options): InternalOptions {
  const opts_ = { ...DEFAULT_OPTIONS, ...opts } as InternalOptions
  opts_.fs_ = opts_.fs ? opts_.fs.promises : ({ ...gfsPromises, lstat: fs.promises.lstat, realpath: fs.promises.realpath } as unknown as FsPromises)
  return opts_
}

/**
 * Try to create shims.
 */
export async function cmdShim (src: string, to: string, opts?: Options): Promise<void> {
  const opts_ = ingestOptions(opts)
  await cmdShim_(src, to, opts_)
}

/**
 * Try to create shims if exists.
 */
export async function cmdShimIfExists (src: string, to: string, opts?: Options): Promise<void> {
  try {
    await cmdShim_(src, to, { ...ingestOptions(opts), requireSource: true })
  } catch {}
}

/**
 * Check whether a shim's content points at the given source path.
 */
function isShimPointingAt (shimContent: string, src: string): boolean {
  return shimContent.includes(`# ${shimTarget(src)}\n`)
}

/**
 * Whether the shell shim was written while its target was missing.
 */
function isShimForMissingTarget (shimContent: string): boolean {
  return shimContent.includes(`${TARGET_MISSING_MARKER}\n`)
}

/**
 * Check whether a shell shim's `NODE_PATH` is `first`, then `middle`, then
 * `last`. An omitted `first` or `last` accepts the shim's own entries at that
 * end, but only where a non-empty `middle` separates them from the rest.
 */
function isShimNodePath (shimContent: string, expected: { first?: string, middle?: string[], last?: string[] }): boolean {
  const first = expected.first == null ? undefined : normalizePathEnvVar([expected.first]).posix
  const last = expected.last == null ? undefined : normalizePathEnvVar(expected.last).posix
  const value = readShNodePath(shimContent)
  if (value == null) return !first && !last
  if (!first && !last) return false
  const rest = stripOuterEntries(value, first, last)
  return rest != null && isMiddleCurrent(rest, normalizePathEnvVar(expected.middle).posix, {
    keepsLeading: first == null,
    keepsTrailing: last == null,
  })
}

function stripOuterEntries (value: string, first: string | undefined, last: string | undefined): string | undefined {
  const afterFirst = first ? stripEntries(value, first, 'start') : value
  if (afterFirst == null || !last) return afterFirst
  return stripEntries(afterFirst, last, 'end')
}

function isMiddleCurrent (rest: string, middle: string, keeps: { keepsLeading: boolean, keepsTrailing: boolean }): boolean {
  if (rest === middle) return true
  if (middle === '') return false
  return (keeps.keepsLeading && rest.endsWith(`:${middle}`)) ||
    (keeps.keepsTrailing && rest.startsWith(`${middle}:`))
}

function stripEntries (value: string, entries: string, end: 'start' | 'end'): string | undefined {
  if (value === entries) return ''
  if (end === 'start') return value.startsWith(`${entries}:`) ? value.slice(entries.length + 1) : undefined
  return value.endsWith(`:${entries}`) ? value.slice(0, -entries.length - 1) : undefined
}

/**
 * The posix form of the shim's own `NODE_PATH` entries.
 */
function readShNodePath (shimContent: string): string | undefined {
  const winStart = shimContent.indexOf('\nelse\n  new_node_path=')
  if (winStart !== -1) {
    const valueStart = winStart + '\nelse\n  new_node_path='.length
    const lineEnd = shimContent.indexOf('\n', valueStart)
    const rest = shimContent.slice(valueStart)
    const raw = readShSingleQuotedWord(rest)?.raw ?? (lineEnd === -1 ? rest : rest.slice(0, lineEnd))
    return unquoteSh(raw)
  }
  const start = shimContent.indexOf(SH_NODE_PATH_EXPORT)
  if (start === -1) {
    return isWindows && shimContent.includes(SH_NODE_PATH_EXPORT) ? '' : undefined
  }
  const valueStart = start + SH_NODE_PATH_EXPORT.length
  const lineEnd = shimContent.indexOf('\n', valueStart)
  const rest = shimContent.slice(valueStart)
  const raw = readShSingleQuotedWord(rest)?.raw ?? (lineEnd === -1 ? rest : rest.slice(0, lineEnd))
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

function readShSingleQuotedWord (value: string): { raw: string } | undefined {
  if (!value.startsWith("'")) return undefined
  let index = 1
  while (index < value.length) {
    if (value.startsWith("'\\''", index)) {
      index += 4
    } else if (value[index] === "'") {
      return { raw: value.slice(0, index + 1) }
    } else {
      index++
    }
  }
  return undefined
}

function isShimBasedirAnchorCurrent (shimContent: string, relativeTarget: string): boolean {
  return shimContent.includes(SH_SHIM_BASEDIR_ABS_PRELUDE) !== path.isAbsolute(relativeTarget)
}

function readShRelativeTarget (shimContent: string): string | undefined {
  const quotedStart = '"$basedir_abs"/'
  const quotedStartIndex = shimContent.indexOf(quotedStart)
  if (quotedStartIndex !== -1) {
    const valueStart = quotedStartIndex + quotedStart.length
    const raw = readShSingleQuotedWord(shimContent.slice(valueStart))
    return raw ? unquoteSh(raw.raw) : undefined
  }
  const marker = '"$basedir_abs/'
  const start = shimContent.indexOf(marker)
  if (start === -1) return undefined
  const valueStart = start + marker.length
  const end = shimContent.indexOf('"', valueStart)
  if (end === -1) return undefined
  return shimContent.slice(valueStart, end)
}

function rm (filePath: string, opts: InternalOptions): Promise<void> {
  return opts.fs_.unlink(filePath).catch(() => {})
}

async function cmdShim_ (src: string, to: string, opts: InternalOptions) {
  const srcRuntimeInfo = await searchScriptRuntime(src, opts)
  await writeShimsPreCommon(to, opts)
  const shShimDir = opts.shShimDir ?? await getShShimDir(src, to, { fs: opts.fs_ })
  return writeAllShims(src, to, srcRuntimeInfo, { ...opts, shShimDir })
}

function findSharedAncestor (startDir: string, src: string): string {
  let ancestor = startDir
  while (!isSubdirOrEqual(ancestor, src)) {
    const parent = path.dirname(ancestor)
    if (parent === ancestor) return startDir
    ancestor = parent
  }
  return ancestor
}

async function getShShimDir (src: string, to: string, opts: GetShShimDirOptions = {}): Promise<string> {
  const dir = path.dirname(to)
  const fs_ = opts.fs ?? fs.promises
  const physicalDir = opts.physicalDir ?? await getPhysicalShimDir(dir, fs_)
  if (physicalDir === dir) return dir
  const ancestor = findSharedAncestor(dir, src)
  if (ancestor === dir && !isSubdirOrEqual(dir, src)) return physicalDir
  const physicalAncestor = await fs_.realpath(ancestor)
  return isSubdirOrEqual(physicalAncestor, physicalDir)
    ? path.join(ancestor, path.relative(physicalAncestor, physicalDir))
    : physicalDir
}

async function getPhysicalShimDir (dir: string, fs_: ShimDirFs = fs.promises): Promise<string> {
  if (isWindows && !await hasLinkOnPath(dir, fs_)) return dir
  return fs_.realpath(dir)
}

async function hasLinkOnPath (dir: string, fs_: ShimDirFs): Promise<boolean> {
  const ancestors = [dir]
  for (let parent = path.dirname(dir); parent !== ancestors.at(-1); parent = path.dirname(parent)) {
    ancestors.push(parent)
  }
  const isLink = await Promise.all(ancestors.map(async (ancestor) => isSymbolicLink(ancestor, fs_)))
  return isLink.includes(true)
}

async function isSymbolicLink (file: string, fs_: ShimDirFs): Promise<boolean> {
  try {
    return (await fs_.lstat(file)).isSymbolicLink()
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
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

async function searchScriptRuntime (target: string, opts: InternalOptions): Promise<RuntimeInfo> {
  let data: string
  try {
    data = await opts.fs_.readFile(target, 'utf8') as string
  } catch (err) {
    if (!isMissingPathError(err)) throw err
    if (isWindows && path.extname(target) === '' && await exists(`${target}${getExeExtension()}`, opts)) {
      return {
        program: null,
        additionalArgs: '',
      }
    }
    if (opts.requireSource) throw err
    return { ...runtimeFromExtension(target), isTargetMissing: true }
  }

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

function runtimeFromExtension (target: string): RuntimeInfo {
  const targetExtension = path.extname(target).toLowerCase()
  const program = extensionToProgramMap.get(targetExtension) || null
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
  return isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')
}

function getExeExtension (): string {
  let cmdExtension
  if (process.env.PATHEXT) {
    cmdExtension = process.env.PATHEXT
      .split(path.delimiter)
      .find((ext) => ext.toLowerCase() === '.exe')
  }
  return cmdExtension || '.exe'
}

async function writeShim (src: string, to: string, srcRuntimeInfo: RuntimeInfo, generateShimScript: ShimGenerator, opts: InternalOptions) {
  const defaultArgs = opts.preserveSymlinks ? '--preserve-symlinks' : ''
  const args = [srcRuntimeInfo.additionalArgs, defaultArgs].filter((arg) => arg).join(' ')
  opts = Object.assign({}, opts, {
    prog: srcRuntimeInfo.program,
    args,
    isTargetMissing: srcRuntimeInfo.isTargetMissing,
  })

  await writeShimPre(to, opts)
  await opts.fs_.writeFile(to, generateShimScript(src, to, opts), 'utf8')
  return writeShimPost(to, opts)
}

function chmodShim (to: string, opts: InternalOptions) {
  return opts.fs_.chmod(to, 0o755)
}

export {
  generateCmdShim,
  generatePwshShim,
  generateShShim,
  getExeExtension,
  getPhysicalShimDir,
  getShShimDir,
  isShimBasedirAnchorCurrent,
  isShimForMissingTarget,
  isShimMsysCmdSwitchDoubled,
  isShimNodePath,
  isShimPointingAt,
  readShNodePath,
  readShRelativeTarget,
}

