import { promises as fs, existsSync } from 'fs'
import { createRequire } from 'module'
import path from 'path'
import { getNodeBinLocationForCurrentOS, getDenoBinLocationForCurrentOS, getBunBinLocationForCurrentOS } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { logger, globalWarn } from '@pnpm/logger'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { type Command, getBinsFromPackageManifest } from '@pnpm/package-bins'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { type EngineDependency, type DependencyManifest, type ProjectManifest } from '@pnpm/types'
import cmdShim from '@zkochan/cmd-shim'
import rimraf from '@zkochan/rimraf'
import isSubdir from 'is-subdir'
import isWindows from 'is-windows'
import normalizePath from 'normalize-path'
import { isEmpty, unnest, groupBy, partition } from 'ramda'
import semver from 'semver'
import symlinkDir from 'symlink-dir'
import fixBin from 'bin-links/lib/fix-bin.js'
import { getBinNodePaths } from './getBinNodePaths.js'

const binsConflictLogger = logger('bins-conflict')
const IS_WINDOWS = isWindows()
const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS
const POWER_SHELL_IS_SUPPORTED = IS_WINDOWS

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
  opts: LinkBinOptions = {}
): Promise<string[]> {
  if (pkgs.length === 0) return []

  const allCmds = unnest(
    (await Promise.all(
      pkgs
        .map(async (pkg) => getPackageBinsFromManifest(pkg.manifest, pkg.location))
    ))
      .filter((cmds: Command[]) => cmds.length)
  )

  return _linkBins(allCmds, binsTarget, opts)
}

interface CommandInfo extends Command {
  ownName: boolean
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
  if (a.ownName && !b.ownName) return 1
  if (!a.ownName && b.ownName) return -1
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
    // There is a probably a better way to do this.
    // It isn't good to have these hardcoded here.
    switch (path.basename(target)) {
    case 'node':
      return [{
        name: 'node',
        path: path.join(target, getNodeBinLocationForCurrentOS()),
        ownName: true,
        pkgName: '',
        pkgVersion: '',
        makePowerShellShim: false,
      }]
    case 'deno':
      return [{
        name: 'deno',
        path: path.join(target, getDenoBinLocationForCurrentOS()),
        ownName: true,
        pkgName: '',
        pkgVersion: '',
        makePowerShellShim: false,
      }]
    case 'bun':
      return [{
        name: 'bun',
        path: path.join(target, getBunBinLocationForCurrentOS()),
        ownName: true,
        pkgName: '',
        pkgVersion: '',
        makePowerShellShim: false,
      }]
    }
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
    ownName: cmd.name === manifest.name,
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
  /**
   * When true, forces regeneration of bin scripts even if they already exist.
   * This is needed for global installations to ensure bin scripts point to the
   * correct version after updating. See: https://github.com/pnpm/pnpm/issues/10517
   */
  global?: boolean
}

async function linkBin (cmd: CommandInfo, binsDir: string, opts?: LinkBinOptions): Promise<void> {
  const externalBinPath = path.join(binsDir, cmd.name)

  // For global installations, remove existing bin scripts to force regeneration.
  // This fixes https://github.com/pnpm/pnpm/issues/10517 where bin scripts
  // contain hardcoded version paths that become stale after updating.
  if (opts?.global && !IS_WINDOWS) {
    if (existsSync(externalBinPath)) {
      await rimraf(externalBinPath)
    }
  }

  if (IS_WINDOWS) {
    const exePath = path.join(binsDir, `${cmd.name}${getExeExtension()}`)
    if (existsSync(exePath)) {
      globalWarn(`The target bin directory already contains an exe called ${cmd.name}, so removing ${exePath}`)
      await rimraf(exePath)
    }
    // node.exe must exist as a real executable, not a cmd-shim wrapper.
    // We could update our own cmd shims to support node.cmd, but we can't
    // control npm's cmd shims, which break when node resolves to node.cmd.
    // npm's cmd shims use `IF EXIST "%~dp0\node.exe"` to find the node binary.
    if (cmd.name === 'node' && cmd.path.toLowerCase().endsWith('.exe')) {
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
    try {
      if (existsSync(externalBinPath)) {
        await rimraf(externalBinPath)
      }
    } catch (err: unknown) {
      if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    }
    await fs.symlink(cmd.path, externalBinPath, 'file')
    return
  }

  if (opts?.preferSymlinkedExecutables && !IS_WINDOWS && cmd.nodeExecPath == null) {
    try {
      await symlinkDir(cmd.path, externalBinPath)
      await fixBin(cmd.path, 0o755)
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
    if (err.code !== 'ENOENT' && err.code !== 'EISDIR') {
      throw err
    }
    globalWarn(`Failed to create bin at ${externalBinPath}. ${err.message as string}`)
    return
  }
  // ensure that bin are executable and not containing
  // windows line-endings(CRLF) on the hashbang line
  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    await fixBin(cmd.path, 0o755)
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
