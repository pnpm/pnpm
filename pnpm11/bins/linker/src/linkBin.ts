import { existsSync, promises as fs } from 'node:fs'
import path from 'node:path'

import { cmdShim, getExeExtension, getShShimDir, isShimBasedirAnchorCurrent, isShimForMissingTarget, isShimNodePath, isShimPointingAt, readShRelativeTarget } from '@pnpm/bins.cmd-shim'
import type { Command } from '@pnpm/bins.resolver'
import { globalWarn } from '@pnpm/logger'
import { rimraf } from '@zkochan/rimraf'
import { symlinkDir } from 'symlink-dir'

import { canSymlinkExecutable, ensureExecutableIfNeeded, isMissing, isPathPresent, isSameFile } from './fileChecks.js'
import { getBinNodePaths } from './getBinNodePaths.js'
import { IS_WINDOWS } from './platform.js'

const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS
const POWER_SHELL_IS_SUPPORTED = IS_WINDOWS
// A cmd-shim is a small shell script. Anything larger is a binary and should not be read.
// A POSIX shim written on Windows lists NODE_PATH in two forms and can pass 4 KiB.
const CMD_SHIM_MAX_SIZE = 64 * 1024

export interface CommandInfo extends Command {
  pkgDir: string
  pkgName: string
  pkgVersion: string
  /**
   * Whether the owning package wants a PowerShell shim next to its .cmd one.
   * The pnpm CLI opts out: PowerShell resolves `pnpm.ps1` ahead of `pnpm.cmd`,
   * so a shim written for one installation of the CLI would keep shadowing
   * every later one, including an upgrade that ships a different executable.
   * Platform support is applied where the shim is created, not here.
   */
  makePowerShellShim: boolean
  nodeExecPath?: string
}

export interface LinkBinOptions {
  /** Rewrite shims after replacing a target with a different interpreter or native binary. */
  force?: boolean
  /** Refresh only winning commands from these normalized absolute package directories. */
  forceForPackages?: ReadonlySet<string>
  /** The command names selected by this bin-link pass. */
  linkedCommandNames?: Set<string>
  /**
   * `NODE_PATH` entries after the bin's own dependency directories. An empty
   * list means none. When omitted, an existing shim keeps its `NODE_PATH`.
   */
  extraNodePaths?: string[]
  /**
   * The first `NODE_PATH` entry: the modules directory of the project whose
   * bins are linked, when Node cannot find it by walking up to `node_modules`.
   */
  projectModulesDir?: string
  preferSymlinkedExecutables?: boolean
  /**
   * Hold back every bin whose target is missing, and remove a shim an earlier
   * install left for it, as a package's own `.bin` always does. For a pass that
   * runs before dependency builds and is repeated after them: a build may
   * create the target, and its scripts run with this `.bin` on PATH.
   */
  holdBackMissingTargets?: boolean
  /**
   * Receives `binsDir` when this pass holds back a bin, so the caller can link
   * the directory again once the builds ran.
   */
  heldBackBinsDirs?: Set<string>
}

interface BinLinkContext {
  cmd: CommandInfo
  binsDir: string
  externalBinPath: string
  shShimDir: string
  opts: LinkBinOptions
}

export async function linkBin (cmd: CommandInfo, binsDir: string, opts: LinkBinOptions & { physicalBinsDir: string }): Promise<void> {
  const externalBinPath = path.join(binsDir, cmd.name)
  const shShimDir = await getShShimDir(cmd.path, externalBinPath, { physicalDir: opts.physicalBinsDir })
  // Not writing a PowerShell shim is not enough to keep one out of the bin
  // directory: an install that did want one leaves it behind, and PowerShell
  // keeps preferring it over the .cmd shim. This runs above the short-circuits
  // below, which all return without touching the .ps1 sibling.
  if (!cmd.makePowerShellShim) {
    await rimraf(`${externalBinPath}.ps1`)
  }
  const ctx: BinLinkContext = { cmd, binsDir, externalBinPath, shShimDir, opts }
  if (!opts.force && !opts.forceForPackages?.has(path.normalize(cmd.pkgDir)) && await isBinCorrectlyLinked(ctx)) {
    // If a previous install failed, we may have re-copied the bin script from
    // the store, but we won't necessarily have reapplied the executable bit -
    // so apply it here.
    if (EXECUTABLE_SHEBANG_SUPPORTED) {
      await ensureExecutableIfNeeded(cmd.path, { allowMissing: true })
    }
    return
  }
  if (await linkNodeExecutable(ctx)) return

  if (opts.preferSymlinkedExecutables && !IS_WINDOWS && cmd.nodeExecPath == null && await canSymlinkExecutable(cmd.path)) {
    await symlinkExecutable(cmd.path, externalBinPath)
    return
  }

  if (!await writeCmdShim(ctx)) return
  // ensure that bin are executable and not containing
  // windows line-endings(CRLF) on the hashbang line
  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    await ensureExecutableIfNeeded(cmd.path, { allowMissing: true })
  }
}

// Skip if the existing bin already references the correct target.
// This avoids redundant I/O on warm installs and EACCES on read-only stores.
// We verify the target path — not just existence — so that conflict resolution
// changes or provider swaps still get the bin rewritten.
async function isBinCorrectlyLinked (ctx: BinLinkContext): Promise<boolean> {
  try {
    const stat = await fs.lstat(ctx.externalBinPath)
    if (stat.isSymbolicLink()) {
      return await isSymlinkCurrent(ctx)
    }
    if (stat.isFile() && stat.size < CMD_SHIM_MAX_SIZE) {
      return await isShimFileCurrent(await fs.readFile(ctx.externalBinPath, 'utf8'), ctx)
    }
  } catch {}
  return false
}

async function isSymlinkCurrent ({ cmd, binsDir, externalBinPath }: BinLinkContext): Promise<boolean> {
  const target = await fs.readlink(externalBinPath)
  if (target !== cmd.path && path.resolve(binsDir, target) !== path.resolve(cmd.path)) return false
  return !EXECUTABLE_SHEBANG_SUPPORTED || canSymlinkExecutable(cmd.path)
}

async function isShimFileCurrent (content: string, ctx: BinLinkContext): Promise<boolean> {
  const { cmd, binsDir, externalBinPath } = ctx
  if (!isShimPointingAt(content, cmd.path) || !isShimHardened(content)) return false
  if (IS_WINDOWS && !existsSync(`${externalBinPath}.cmd`)) return false
  if (isShimForMissingTarget(content) && !await isMissing(cmd.path)) return false
  return isShimBasedirAnchorCurrent(content, path.relative(binsDir, cmd.path)) &&
    isShimRelativeTargetCurrent(content, ctx) &&
    isShimNodePathCurrent(content, ctx.opts)
}

function isShimRelativeTargetCurrent (content: string, { cmd, shShimDir }: BinLinkContext): boolean {
  const expectedRelativeTarget = path.relative(shShimDir, cmd.path).split('\\').join('/')
  const storedRelativeTarget = readShRelativeTarget(content)
  return path.isAbsolute(expectedRelativeTarget)
    ? storedRelativeTarget == null
    : storedRelativeTarget === expectedRelativeTarget
}

function isShimNodePathCurrent (content: string, opts: LinkBinOptions): boolean {
  if (opts.extraNodePaths == null && opts.projectModulesDir == null) return true
  return isShimNodePath(content, {
    first: opts.projectModulesDir,
    // The shim lists every entry once, at its first position.
    last: opts.extraNodePaths && Array.from(new Set(opts.extraNodePaths)).filter((nodePath) => nodePath !== opts.projectModulesDir),
  })
}

/**
 * Links `node` as a real executable, the way npm's shims and PATH lookups
 * expect it.
 *
 * @returns `true` when the bin is handled and no shim must be written.
 */
async function linkNodeExecutable ({ cmd, binsDir, externalBinPath }: BinLinkContext): Promise<boolean> {
  if (IS_WINDOWS) return linkWindowsExe(cmd, binsDir)
  if (cmd.name !== 'node') return false
  // On non-Windows, node should be symlinked directly to the binary
  // instead of wrapped in a shell shim.
  // Use rimraf unconditionally instead of existsSync check, because
  // existsSync follows symlinks and returns false for broken symlinks,
  // causing EEXIST when the dangling symlink still exists on disk.
  await rimraf(externalBinPath)
  await fs.symlink(cmd.path, externalBinPath, 'file')
  return true
}

async function linkWindowsExe (cmd: CommandInfo, binsDir: string): Promise<boolean> {
  const exePath = path.join(binsDir, `${cmd.name}${getExeExtension()}`)
  // node.exe is the only bin pnpm links directly as a real executable rather
  // than through a cmd-shim, so the existing-exe handling only applies to it.
  // We could update our own cmd shims to support node.cmd, but we can't
  // control npm's cmd shims, which break when node resolves to node.cmd.
  // npm's cmd shims use `IF EXIST "%~dp0\node.exe"` to find the node binary.
  const isNodeExe = cmd.name === 'node' && cmd.path.toLowerCase().endsWith('.exe')
  // A dangling symlink must be removed too, so the check can't follow links.
  if (await isPathPresent(exePath)) {
    // Skip warning and re-linking when the existing node.exe already matches
    // the target, otherwise every command that re-links node would spam the
    // warning below on warm installs.
    if (isNodeExe && await isSameFile(exePath, cmd.path)) {
      return true
    }
    globalWarn(`The target bin directory already contains an exe called ${cmd.name}, so removing ${exePath}`)
    await rimraf(exePath)
  }
  if (!isNodeExe) return false
  try {
    await fs.link(cmd.path, exePath)
  } catch {
    await fs.copyFile(cmd.path, exePath)
  }
  return true
}

async function symlinkExecutable (target: string, externalBinPath: string): Promise<void> {
  try {
    await symlinkDir(target, externalBinPath)
    await ensureExecutableIfNeeded(target, { allowMissing: true })
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT' && err.code !== 'EISDIR') {
      throw err
    }
    globalWarn(`Failed to create bin at ${externalBinPath}. ${err.message as string}`)
  }
}

/**
 * @returns `false` when the shim could not be written and a warning was
 * printed instead.
 */
async function writeCmdShim ({ cmd, externalBinPath, shShimDir, opts }: BinLinkContext): Promise<boolean> {
  try {
    const nodePath = await getShimNodePath(cmd.path, opts)
    await cmdShim(cmd.path, externalBinPath, {
      createPwshFile: POWER_SHELL_IS_SUPPORTED && cmd.makePowerShellShim,
      nodePath,
      nodeExecPath: cmd.nodeExecPath,
      shShimDir,
    })
  } catch (err: any) { // eslint-disable-line
    if (!isSkippableShimError(err)) throw err
    globalWarn(`Failed to create bin at ${externalBinPath}. ${err.message as string}`)
    return false
  }
  return true
}

async function getShimNodePath (target: string, opts: LinkBinOptions): Promise<string[] | undefined> {
  if (!opts.extraNodePaths?.length && !opts.projectModulesDir) return undefined
  return Array.from(new Set([
    ...(opts.projectModulesDir ? [opts.projectModulesDir] : []),
    ...await getBinNodePaths(target, opts.projectModulesDir),
    ...opts.extraNodePaths ?? [],
  ]))
}

function isSkippableShimError (err: NodeJS.ErrnoException): boolean {
  if (err.code === 'ENOENT' || err.code === 'EISDIR') return true
  // On Windows, EPERM during bin creation can happen when another process
  // (e.g. a parallel dlx call) is writing to the same shared bin directory.
  // The other process will finish creating the bin, so we can safely skip.
  return IS_WINDOWS && err.code === 'EPERM'
}

// The target marker does not describe the header, so a shim whose target has
// not moved can still need replacing. Keep these in step with pacquet's
// `is_sh_shim_hardened`.
const SH_SHIM_HARDENED_LINES = [
  '  target=$(run_helper readlink "$link")\n',
  String.raw`basedir=$(run_helper printf '%s\n' "$link" | run_helper sed -e 's,\\,/,g')` + '\n',
  '    if converted=$(command -p cygpath -w "$basedir" 2>/dev/null) && [ -n "$converted" ]; then\n',
  '    if converted=$(command -p wslpath -w "$basedir" 2>/dev/null) && [ -n "$converted" ]; then\n',
  '    */node_modules/*|*/node_modules) ;;\n',
]

function isShimHardened (content: string): boolean {
  return SH_SHIM_HARDENED_LINES.every((line) => content.includes(line))
}
