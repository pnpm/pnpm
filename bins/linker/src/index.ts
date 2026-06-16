import { existsSync, promises as fs } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import { type Command, getBinsFromPackageManifest, pkgOwnsBin } from '@pnpm/bins.resolver'
import { PnpmError } from '@pnpm/error'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { globalWarn, logger } from '@pnpm/logger'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import type { DependencyManifest, EngineDependency, ProjectManifest } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { cmdShim, isShimPointingAt } from '@zkochan/cmd-shim'
import { rimraf } from '@zkochan/rimraf'
import fixBin from 'bin-links/lib/fix-bin.js'
import { isSubdir } from 'is-subdir'
import isWindows from 'is-windows'
import normalizePath from 'normalize-path'
import { groupBy, isEmpty, partition, unnest } from 'ramda'
import semver from 'semver'
import { symlinkDir } from 'symlink-dir'

import { getBinNodePaths } from './getBinNodePaths.js'

const binsConflictLogger = logger('bins-conflict')
const IS_WINDOWS = isWindows()
const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS
const POWER_SHELL_IS_SUPPORTED = IS_WINDOWS
// A cmd-shim is a small shell script. Anything larger is a binary and should not be read.
const CMD_SHIM_MAX_SIZE = 4 * 1024

export type WarningCode = 'BINARIES_CONFLICT' | 'EMPTY_BIN'

export type WarnFunction = (msg: string, code: WarningCode) => void

export async function linkBins (
  modulesDir: string,
  binsDir: string,
  opts: LinkBinOptions & {
    allowExoticManifests?: boolean
    projectManifest?: ProjectManifest
    warn: WarnFunction
  }
): Promise<string[]> {
  const allDeps = await readModulesDir(modulesDir)
  // If the modules dir does not exist, do nothing
  if (allDeps === null) return []
  return linkBinsOfPkgsByAliases(allDeps, binsDir, {
    ...opts,
    modulesDir,
  })
}

export async function linkBinsOfPkgsByAliases (
  depsAliases: string[],
  binsDir: string,
  opts: LinkBinOptions & {
    modulesDir: string
    allowExoticManifests?: boolean
    projectManifest?: ProjectManifest
    warn: WarnFunction
  }
): Promise<string[]> {
  const pkgBinOpts = {
    allowExoticManifests: false,
    ...opts,
  }
  const directDependencies = opts.projectManifest == null
    ? undefined
    : new Set(Object.keys(getAllDependenciesFromManifest(opts.projectManifest)))
  const allCmds = unnest(
    (await Promise.all(
      depsAliases
        .map((alias) => ({
          depDir: path.resolve(opts.modulesDir, alias),
          isDirectDependency: directDependencies?.has(alias),
        }))
        .filter(({ depDir }) => !isSubdir(depDir, binsDir)) // Don't link own bins
        .map(async ({ depDir, isDirectDependency }) => {
          const target = normalizePath(depDir)
          const cmds = await getPackageBins(pkgBinOpts, target)
          return cmds.map((cmd) => ({ ...cmd, isDirectDependency }))
        })
    ))
      .filter((cmds: Command[]) => cmds.length)
  )

  const cmdsToLink = directDependencies != null ? preferDirectCmds(allCmds) : allCmds
  return _linkBins(cmdsToLink, binsDir, opts)
}

function preferDirectCmds (allCmds: Array<CommandInfo & { isDirectDependency?: boolean }>) {
  const [directCmds, hoistedCmds] = partition((cmd) => cmd.isDirectDependency === true, allCmds)
  const usedDirectCmds = new Set(directCmds.map((directCmd) => directCmd.name))
  return [
    ...directCmds,
    ...hoistedCmds.filter(({ name }) => !usedDirectCmds.has(name)),
  ]
}

export async function linkBinsOfPackages (
  pkgs: Array<{
    manifest: DependencyManifest
    location: string
  }>,
  binsTarget: string,
  opts: LinkBinOptions & { excludeBins?: Set<string> } = {}
): Promise<string[]> {
  if (pkgs.length === 0) return []

  let allCmds = unnest(
    (await Promise.all(
      pkgs
        .map(async (pkg) => getPackageBinsFromManifest(pkg.manifest, pkg.location))
    ))
      .filter((cmds: Command[]) => cmds.length)
  )
  const excludeBins = opts.excludeBins
  if (excludeBins?.size) {
    allCmds = allCmds.filter((cmd) => !excludeBins.has(cmd.name))
  }

  return _linkBins(allCmds, binsTarget, opts)
}

interface CommandInfo extends Command {
  pkgName: string
  pkgVersion: string
  makePowerShellShim: boolean
  nodeExecPath?: string
}

async function _linkBins (
  allCmds: CommandInfo[],
  binsDir: string,
  opts: LinkBinOptions
): Promise<string[]> {
  if (allCmds.length === 0) return [] as string[]

  // deduplicate bin names to prevent race conditions (multiple writers for the same file)
  allCmds = deduplicateCommands(allCmds, binsDir)

  await fs.mkdir(binsDir, { recursive: true })

  const results = await Promise.allSettled(allCmds.map(async cmd => linkBin(cmd, binsDir, opts)))

  // We want to create all commands that we can create before throwing an exception
  for (const result of results) {
    if (result.status === 'rejected') {
      throw result.reason
    }
  }

  return allCmds.map(cmd => cmd.pkgName)
}

function deduplicateCommands (commands: CommandInfo[], binsDir: string): CommandInfo[] {
  const cmdGroups = groupBy(cmd => cmd.name, commands)
  return Object.values(cmdGroups)
    .filter((group): group is CommandInfo[] => group !== undefined && group.length !== 0)
    .map(group => resolveCommandConflicts(group, binsDir))
}

function resolveCommandConflicts (group: CommandInfo[], binsDir: string): CommandInfo {
  return group.reduce((a, b) => {
    const [chosen, skipped] = compareCommandsInConflict(a, b) >= 0 ? [a, b] : [b, a]
    logCommandConflict(chosen, skipped, binsDir)
    return chosen
  })
}

function compareCommandsInConflict (a: CommandInfo, b: CommandInfo): number {
  // Check ownership: a package that owns the bin name gets priority
  const aOwns = pkgOwnsBin(a.name, a.pkgName)
  const bOwns = pkgOwnsBin(b.name, b.pkgName)
  if (aOwns && !bOwns) return 1
  if (!aOwns && bOwns) return -1
  if (a.pkgName !== b.pkgName) return a.pkgName.localeCompare(b.pkgName) // it's pointless to compare versions of 2 different package
  return semver.compare(a.pkgVersion, b.pkgVersion)
}

function logCommandConflict (chosen: CommandInfo, skipped: CommandInfo, binsDir: string): void {
  binsConflictLogger.debug({
    binaryName: skipped.name,
    binsDir,
    linkedPkgName: chosen.pkgName,
    linkedPkgVersion: chosen.pkgVersion,
    skippedPkgName: skipped.pkgName,
    skippedPkgVersion: skipped.pkgVersion,
  })
}

async function isFromModules (filename: string): Promise<boolean> {
  const real = await fs.realpath(filename)
  return normalizePath(real).includes('/node_modules/')
}

async function getPackageBins (
  opts: {
    allowExoticManifests: boolean
    warn: WarnFunction
  },
  target: string
): Promise<CommandInfo[]> {
  const manifest = opts.allowExoticManifests
    ? (await safeReadProjectManifestOnly(target) as DependencyManifest)
    : await safeReadPkgJson(target)

  if (manifest == null) {
    // There's a directory in node_modules without package.json: ${target}.
    // This used to be a warning but it didn't really cause any issues.
    return []
  }

  if (isEmpty(manifest.bin) && !await isFromModules(target)) {
    opts.warn(`Package in ${target} must have a non-empty bin field to get bin linked.`, 'EMPTY_BIN')
  }

  if (typeof manifest.bin === 'string' && !manifest.name) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package in ${target} must have a name to get bin linked.`)
  }

  return getPackageBinsFromManifest(manifest, target)
}

async function getPackageBinsFromManifest (manifest: DependencyManifest, pkgDir: string): Promise<CommandInfo[]> {
  const cmds = await getBinsFromPackageManifest(manifest, pkgDir)
  let nodeExecPath: string | undefined
  if (manifest.engines?.runtime && runtimeHasNodeDownloaded(manifest.engines.runtime)) {
    const require = createRequire(import.meta.dirname)
    // Using Node.js’ resolution algorithm is the most reliable way to find the Node.js
    // package that comes from this CLI's dependencies, because the layout of node_modules can vary.
    // In an isolated layout, it will be located in the same node_modules directory as the CLI.
    // In a hoisted layout, it may be in one of the parent node_modules directories.
    const nodeDir = path.dirname(require.resolve('node/CHANGELOG.md', { paths: [pkgDir] }))
    if (nodeDir) {
      nodeExecPath = path.join(nodeDir, IS_WINDOWS ? 'node.exe' : 'bin/node')
    }
  }
  return cmds.map((cmd) => ({
    ...cmd,
    pkgName: manifest.name,
    pkgVersion: manifest.version,
    makePowerShellShim: POWER_SHELL_IS_SUPPORTED && manifest.name !== 'pnpm',
    nodeExecPath,
  }))
}

function runtimeHasNodeDownloaded (runtime: EngineDependency | EngineDependency[]): boolean {
  if (!Array.isArray(runtime)) {
    return runtime.name === 'node' && runtime.onFail === 'download'
  }
  return runtime.find(({ name }) => name === 'node')?.onFail === 'download'
}

export interface LinkBinOptions {
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
}

async function linkBin (cmd: CommandInfo, binsDir: string, opts?: LinkBinOptions): Promise<void> {
  const externalBinPath = path.join(binsDir, cmd.name)
  // Skip if the existing bin already references the correct target.
  // This avoids redundant I/O on warm installs and EACCES on read-only stores.
  // We verify the target path — not just existence — so that conflict resolution
  // changes or provider swaps still get the bin rewritten.
  try {
    const stat = await fs.lstat(externalBinPath)
    if (stat.isSymbolicLink()) {
      const target = await fs.readlink(externalBinPath)
      if (target === cmd.path || path.resolve(binsDir, target) === path.resolve(cmd.path)) {
        return
      }
    } else if (stat.isFile() && stat.size < CMD_SHIM_MAX_SIZE) {
      const content = await fs.readFile(externalBinPath, 'utf8')
      if (isShimPointingAt(content, cmd.path)) {
        return
      }
    }
  } catch {}
  if (IS_WINDOWS) {
    const exePath = path.join(binsDir, `${cmd.name}${getExeExtension()}`)
    // node.exe is the only bin pnpm links directly as a real executable rather
    // than through a cmd-shim, so the existing-exe handling only applies to it.
    // We could update our own cmd shims to support node.cmd, but we can't
    // control npm's cmd shims, which break when node resolves to node.cmd.
    // npm's cmd shims use `IF EXIST "%~dp0\node.exe"` to find the node binary.
    const isNodeExe = cmd.name === 'node' && cmd.path.toLowerCase().endsWith('.exe')
    if (existsSync(exePath)) {
      // Skip warning and re-linking when the existing node.exe already matches
      // the target, otherwise every command that re-links node would spam the
      // warning below on warm installs.
      if (isNodeExe && await isSameFile(exePath, cmd.path)) {
        return
      }
      globalWarn(`The target bin directory already contains an exe called ${cmd.name}, so removing ${exePath}`)
      await rimraf(exePath)
    }
    if (isNodeExe) {
      try {
        await fs.link(cmd.path, exePath)
      } catch {
        await fs.copyFile(cmd.path, exePath)
      }
      return
    }
  } else if (cmd.name === 'node') {
    // On non-Windows, node should be symlinked directly to the binary
    // instead of wrapped in a shell shim.
    // Use rimraf unconditionally instead of existsSync check, because
    // existsSync follows symlinks and returns false for broken symlinks,
    // causing EEXIST when the dangling symlink still exists on disk.
    await rimraf(externalBinPath)
    await fs.symlink(cmd.path, externalBinPath, 'file')
    return
  }

  if (opts?.preferSymlinkedExecutables && !IS_WINDOWS && cmd.nodeExecPath == null) {
    try {
      await symlinkDir(cmd.path, externalBinPath)
      await ensureExecutable(cmd.path, 0o755)
    } catch (err: any) { // eslint-disable-line
      if (err.code !== 'ENOENT' && err.code !== 'EISDIR') {
        throw err
      }
      globalWarn(`Failed to create bin at ${externalBinPath}. ${err.message as string}`)
    }
    return
  }

  try {
    let nodePath: string[] | undefined
    if (opts?.extraNodePaths?.length) {
      const binNodePaths = await getBinNodePaths(cmd.path)
      if (binNodePaths.length === 0) {
        nodePath = opts.extraNodePaths
      } else {
        nodePath = [...binNodePaths]
        for (const p of opts.extraNodePaths) {
          if (!binNodePaths.includes(p)) {
            nodePath.push(p)
          }
        }
      }
    }
    await cmdShim(cmd.path, externalBinPath, {
      createPwshFile: cmd.makePowerShellShim,
      nodePath,
      nodeExecPath: cmd.nodeExecPath,
    })
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT' || err.code === 'EISDIR') {
      globalWarn(`Failed to create bin at ${externalBinPath}. ${err.message as string}`)
      return
    }
    // On Windows, EPERM during bin creation can happen when another process
    // (e.g. a parallel dlx call) is writing to the same shared bin directory.
    // The other process will finish creating the bin, so we can safely skip.
    if (IS_WINDOWS && err.code === 'EPERM') {
      globalWarn(`Failed to create bin at ${externalBinPath}. ${err.message as string}`)
      return
    }
    throw err
  }
  // ensure that bin are executable and not containing
  // windows line-endings(CRLF) on the hashbang line
  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    await ensureExecutable(cmd.path, 0o755)
  }
}

// Reports whether two paths refer to the same file. A matching inode/device
// pair (read as BigInts to avoid the precision loss of NTFS 64-bit file IDs)
// proves a hard link cheaply. Whenever identity can't be established that way —
// because the inodes genuinely differ or because Windows reports an unreliable
// zero inode — we fall back to comparing the file contents after a quick size
// check, which also treats a byte-identical copy as the same file.
async function isSameFile (pathA: string, pathB: string): Promise<boolean> {
  const [statA, statB] = await Promise.all([
    fs.stat(pathA, { bigint: true }).catch(() => null),
    fs.stat(pathB, { bigint: true }).catch(() => null),
  ])
  if (statA == null || statB == null) return false
  if (statA.ino && statB.ino && statA.ino === statB.ino && statA.dev === statB.dev) {
    return true
  }
  if (statA.size !== statB.size) return false
  return haveEqualContents(pathA, pathB)
}

const FILE_COMPARE_CHUNK_SIZE = 64 * 1024

// Compares two equally-sized files chunk by chunk, so an executable is never
// fully buffered in memory and a mismatch returns as early as possible.
async function haveEqualContents (pathA: string, pathB: string): Promise<boolean> {
  const [fhA, fhB] = await Promise.all([
    fs.open(pathA, 'r').catch(() => null),
    fs.open(pathB, 'r').catch(() => null),
  ])
  if (fhA == null || fhB == null) {
    await fhA?.close().catch(() => {})
    await fhB?.close().catch(() => {})
    return false
  }
  try {
    const bufA = Buffer.alloc(FILE_COMPARE_CHUNK_SIZE)
    const bufB = Buffer.alloc(FILE_COMPARE_CHUNK_SIZE)
    let position = 0
    for (;;) {
      // Reading sequentially is intentional: each iteration compares one chunk
      // and stops early on a mismatch or EOF.
      const [readA, readB] = await Promise.all([ // eslint-disable-line no-await-in-loop
        fhA.read(bufA, 0, FILE_COMPARE_CHUNK_SIZE, position),
        fhB.read(bufB, 0, FILE_COMPARE_CHUNK_SIZE, position),
      ])
      if (readA.bytesRead !== readB.bytesRead) return false
      if (readA.bytesRead === 0) return true
      if (!bufA.subarray(0, readA.bytesRead).equals(bufB.subarray(0, readB.bytesRead))) {
        return false
      }
      position += readA.bytesRead
    }
  } catch {
    // A transient read error must not abort bin linking: treat the files as
    // different so the caller falls back to the warn + remove + relink path.
    return false
  } finally {
    await fhA.close().catch(() => {})
    await fhB.close().catch(() => {})
  }
}

// `fixBin` chmods the bin's source file (which lives inside the store) to make
// it executable and rewrites a Windows CRLF shebang to LF. Under the global
// virtual store that source is `{storeDir}/links/...`, so on a read-only store
// (e.g. `frozenStore`) the chmod is refused — with EPERM/EACCES when the file is
// owned but permissions forbid it, or EROFS on a genuinely read-only filesystem
// (Nix store, RO bind mount, OCI layer). A complete seed already ships its bins
// executable and shebang-normalized by the writable seed-build, so that work is
// redundant: treat an already-correct target as a no-op, keeping bin-linking
// write-free (see building/during-install: "Bin-linking reuses existing symlinks
// write-free"). A non-executable bin — or one still carrying a CRLF shebang that
// `fixBin` could not rewrite here — still throws, because that means the seed is
// broken and the bin would not run.
async function ensureExecutable (file: string, mode: number): Promise<void> {
  try {
    await fixBin(file, mode)
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'EPERM' || err.code === 'EACCES' || err.code === 'EROFS') {
      const stat = await fs.stat(file).catch(() => undefined)
      if (stat != null && (stat.mode & 0o111) !== 0 && !(await hasWindowsShebang(file))) return
    }
    throw err
  }
}

// Detects a `#!`-shebang line terminated by CRLF, which fails to execute on
// POSIX. Mirrors bin-links' own fix-bin detection so a chmod failure on a
// read-only store is only swallowed when the bin is genuinely already correct.
async function hasWindowsShebang (file: string): Promise<boolean> {
  const fh = await fs.open(file, 'r').catch(() => undefined)
  if (fh == null) return false
  try {
    const buf = Buffer.alloc(2048)
    await fh.read(buf, 0, 2048, 0)
    return buf[0] === 0x23 /* # */ && buf[1] === 0x21 /* ! */ && /^#![^\n]+\r\n/.test(buf.toString())
  } catch {
    return false
  } finally {
    await fh.close().catch(() => {})
  }
}

function getExeExtension (): string {
  let cmdExtension

  if (process.env.PATHEXT) {
    cmdExtension = process.env.PATHEXT
      .split(path.delimiter)
      .find(ext => ext.toUpperCase() === '.EXE')
  }

  return cmdExtension ?? '.exe'
}

async function safeReadPkgJson (pkgDir: string): Promise<DependencyManifest | null> {
  try {
    return await readPackageJsonFromDir(pkgDir) as DependencyManifest
  } catch (err: any) { // eslint-disable-line
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      return null
    }
    throw err
  }
}
