import { promises as fs, existsSync } from 'fs'
import Module from 'module'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { logger, globalWarn } from '@pnpm/logger'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { type Command, getBinsFromPackageManifest } from '@pnpm/package-bins'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { type DependencyManifest, type ProjectManifest } from '@pnpm/types'
import cmdShim from '@zkochan/cmd-shim'
import rimraf from '@zkochan/rimraf'
import isSubdir from 'is-subdir'
import isWindows from 'is-windows'
import normalizePath from 'normalize-path'
import pSettle from 'p-settle'
import isEmpty from 'ramda/src/isEmpty'
import unnest from 'ramda/src/unnest'
import groupBy from 'ramda/src/groupBy'
import partition from 'ramda/src/partition'
import semver from 'semver'
import symlinkDir from 'symlink-dir'
import fixBin from 'bin-links/lib/fix-bin'

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
    nodeExecPathByAlias?: Record<string, string>
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
    nodeExecPathByAlias?: Record<string, string>
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
          nodeExecPath: opts.nodeExecPathByAlias?.[alias],
        }))
        .filter(({ depDir }) => !isSubdir(depDir, binsDir)) // Don't link own bins
        .map(async ({ depDir, isDirectDependency, nodeExecPath }) => {
          const target = normalizePath(depDir)
          const cmds = await getPackageBins(pkgBinOpts, target, nodeExecPath)
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
    nodeExecPath?: string
    location: string
  }>,
  binsTarget: string,
  opts: LinkBinOptions = {}
): Promise<string[]> {
  if (pkgs.length === 0) return []

  const allCmds = unnest(
    (await Promise.all(
      pkgs
        .map(async (pkg) => getPackageBinsFromManifest(pkg.manifest, pkg.location, pkg.nodeExecPath))
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

  const results = await pSettle(allCmds.map(async cmd => linkBin(cmd, binsDir, opts)))

  // We want to create all commands that we can create before throwing an exception
  for (const result of results) {
    if (result.isRejected) {
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
  target: string,
  nodeExecPath?: string
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

  return getPackageBinsFromManifest(manifest, target, nodeExecPath)
}

async function getPackageBinsFromManifest (manifest: DependencyManifest, pkgDir: string, nodeExecPath?: string): Promise<CommandInfo[]> {
  const cmds = await getBinsFromPackageManifest(manifest, pkgDir)
  return cmds.map((cmd) => ({
    ...cmd,
    ownName: cmd.name === manifest.name,
    pkgName: manifest.name,
    pkgVersion: manifest.version,
    makePowerShellShim: POWER_SHELL_IS_SUPPORTED && manifest.name !== 'pnpm',
    nodeExecPath,
  }))
}

export interface LinkBinOptions {
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
}

async function linkBin (cmd: CommandInfo, binsDir: string, opts?: LinkBinOptions): Promise<void> {
  const externalBinPath = path.join(binsDir, cmd.name)
  if (IS_WINDOWS) {
    const exePath = path.join(binsDir, `${cmd.name}${getExeExtension()}`)
    if (existsSync(exePath)) {
      globalWarn(`The target bin directory already contains an exe called ${cmd.name}, so removing ${exePath}`)
      await rimraf(exePath)
    }
  }

  if (opts?.preferSymlinkedExecutables && !IS_WINDOWS && cmd.nodeExecPath == null) {
    try {
      await symlinkDir(cmd.path, externalBinPath)
      await fixBin(cmd.path, 0o755)
    } catch (err: any) { // eslint-disable-line
      if (err.code !== 'ENOENT') {
        throw err
      }
      globalWarn(`Failed to create bin at ${externalBinPath}. ${err.message as string}`)
    }
    return
  }

  try {
    let nodePath: string[] | undefined
    if (opts?.extraNodePaths?.length) {
      nodePath = []
      for (const modulesPath of await getBinNodePaths(cmd.path)) {
        if (opts.extraNodePaths.includes(modulesPath)) break
        nodePath.push(modulesPath)
      }
      nodePath.push(...opts.extraNodePaths)
    }
    await cmdShim(cmd.path, externalBinPath, {
      createPwshFile: cmd.makePowerShellShim,
      nodePath,
      nodeExecPath: cmd.nodeExecPath,
    })
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') {
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

async function getBinNodePaths (target: string): Promise<string[]> {
  const targetDir = path.dirname(target)
  try {
    const targetRealPath = await fs.realpath(targetDir)
    // @ts-expect-error
    return Module['_nodeModulePaths'](targetRealPath)
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') {
      throw err
    }
    // @ts-expect-error
    return Module['_nodeModulePaths'](targetDir)
  }
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
