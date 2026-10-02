import { promises as fs } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import { getExeExtension, getPhysicalShimDir } from '@pnpm/bins.cmd-shim'
import { type Command, getBinsFromPackageManifest, pkgOwnsBin } from '@pnpm/bins.resolver'
import { PnpmError } from '@pnpm/error'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { logger } from '@pnpm/logger'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import type { DependencyManifest, EngineDependency, ProjectManifest } from '@pnpm/types'
import { safeReadParentPublishManifest, safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir } from 'is-subdir'
import normalizePath from 'normalize-path'
import { groupBy, isEmpty, partition, unnest } from 'ramda'
import semver from 'semver'

import { isMissing } from './fileChecks.js'
import { type CommandInfo, linkBin, type LinkBinOptions } from './linkBin.js'
import { IS_WINDOWS } from './platform.js'

export { getProjectNodePath } from './getProjectNodePath.js'
export type { LinkBinOptions } from './linkBin.js'

const binsConflictLogger = logger('bins-conflict')

/**
 * The directory holding the `node` executable of the Node.js runtime package
 * installed at `nodeDir`: `nodeDir` itself on Windows, its `bin` directory
 * elsewhere.
 */
export function nodeRuntimeBinDir (nodeDir: string): string {
  return IS_WINDOWS ? nodeDir : path.join(nodeDir, 'bin')
}

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
  return _linkBins(await getCommandsByAliases(depsAliases, binsDir, opts), binsDir, opts)
}

/** Return a function that refreshes launchers for completed package roots,
 * including removing commands those packages no longer provide. */
export async function createBinRefreshPlan (modulesDir: string, binsDir: string, opts: { warn: WarnFunction }): Promise<(pkgRoots: ReadonlySet<string>) => Promise<void>> {
  const candidates = await readBinCandidates(modulesDir, binsDir, opts)
  return async pkgRoots => {
    const changedNames = new Set(Array.from(pkgRoots).flatMap(pkgRoot => (candidates.get(pkgRoot) ?? []).map(cmd => cmd.name)))
    await Promise.all(Array.from(pkgRoots, async pkgRoot => {
      const refreshed = await getPackageBins({ ...opts, allowExoticManifests: false }, normalizePath(pkgRoot))
      for (const cmd of refreshed) changedNames.add(cmd.name)
      candidates.set(pkgRoot, refreshed)
    }))
    const allWinners = deduplicateCommands(Array.from(candidates.values()).flat())
    const winners = allWinners.filter(cmd => changedNames.has(cmd.name))
    const retainedNames = new Set(winners.map(cmd => cmd.name))
    const removedNames = Array.from(changedNames).filter(name => !retainedNames.has(name))
    await Promise.all(removedNames.map(async name => removeBin(path.join(binsDir, name))))
    await _linkBins(IS_WINDOWS && removedNames.length > 0 ? allWinners : winners, binsDir, { forceForPackages: pkgRoots })
  }
}

async function readBinCandidates (modulesDir: string, binsDir: string, opts: { warn: WarnFunction }): Promise<Map<string, CommandInfo[]>> {
  const aliases = await readModulesDir(modulesDir) ?? []
  const candidates = new Map(aliases.map(alias => [path.normalize(path.resolve(modulesDir, alias)), [] as CommandInfo[]]))
  for (const command of await getCommandsByAliases(aliases, binsDir, { ...opts, modulesDir })) {
    candidates.get(path.normalize(command.pkgDir))!.push(command)
  }
  return candidates
}

async function getCommandsByAliases (
  depsAliases: string[],
  binsDir: string,
  opts: Parameters<typeof linkBinsOfPkgsByAliases>[2]
): Promise<CommandInfo[]> {
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
  return cmdsToLink
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
  return _linkBins(await getCommandsToLink(pkgs, opts.excludeBins), binsTarget, opts)
}

export async function getBinsToLink (
  pkgs: Array<{
    manifest: DependencyManifest
    location: string
  }>,
  excludeBins: Set<string> = new Set()
): Promise<Command[]> {
  return deduplicateCommands(await getCommandsToLink(pkgs, excludeBins))
    .map(({ name, path }) => ({ name, path }))
}

async function getCommandsToLink (
  pkgs: Array<{
    manifest: DependencyManifest
    location: string
  }>,
  excludeBins: Set<string> = new Set()
): Promise<CommandInfo[]> {
  const excluded = IS_WINDOWS ? new Set(Array.from(excludeBins, (name) => name.toLowerCase())) : excludeBins
  return unnest(
    (await Promise.all(
      pkgs.map(async (pkg) => getPackageBinsFromManifest(pkg.manifest, pkg.location))
    ))
      .filter((cmds: Command[]) => cmds.length)
  ).filter((cmd) => !excluded.has(IS_WINDOWS ? cmd.name.toLowerCase() : cmd.name))
}

async function _linkBins (
  allCmds: CommandInfo[],
  binsDir: string,
  opts: LinkBinOptions
): Promise<string[]> {
  if (allCmds.length === 0) return [] as string[]

  // deduplicate bin names to prevent race conditions (multiple writers for the same file)
  allCmds = deduplicateCommands(allCmds, binsDir)
  for (const cmd of allCmds) opts.linkedCommandNames?.add(cmd.name)

  await fs.mkdir(binsDir, { recursive: true })
  const physicalBinsDir = await getPhysicalShimDir(binsDir)

  // Removals finish before any shim is written: on Windows the siblings of a
  // removed bin `tool` include `tool.cmd`, which may be another bin's shim.
  const removals = await Promise.allSettled(allCmds.map(async (cmd) => removeBinIfTargetAwaited(cmd, binsDir, opts)))
  const cmdsToLink = allCmds.filter((_, i) => removals[i].status === 'fulfilled' && !removals[i].value)
  if (cmdsToLink.length < allCmds.length) opts.heldBackBinsDirs?.add(binsDir)
  const results = await Promise.allSettled(cmdsToLink.map(async cmd => linkBin(cmd, binsDir, { ...opts, physicalBinsDir })))

  // We want to create all commands that we can create before throwing an exception
  for (const result of [...removals, ...results]) {
    if (result.status === 'rejected') {
      throw result.reason
    }
  }

  return allCmds.map(cmd => cmd.pkgName)
}

function deduplicateCommands (commands: CommandInfo[], binsDir?: string): CommandInfo[] {
  const cmdGroups = groupBy(cmd => cmd.name, commands)
  return Object.values(cmdGroups)
    .filter((group): group is CommandInfo[] => group !== undefined && group.length !== 0)
    .map(group => resolveCommandConflicts(group, binsDir))
}

function resolveCommandConflicts (group: CommandInfo[], binsDir?: string): CommandInfo {
  return group.reduce((chosenSoFar, candidate) => {
    const [chosen, skipped] = compareCommandsInConflict(chosenSoFar, candidate) >= 0 ? [chosenSoFar, candidate] : [candidate, chosenSoFar]
    if (binsDir != null) logCommandConflict(chosen, skipped, binsDir)
    return chosen
  })
}

function compareCommandsInConflict (left: CommandInfo, right: CommandInfo): number {
  // Check ownership: a package that owns the bin name gets priority
  const leftOwns = pkgOwnsBin(left.name, left.pkgName)
  const rightOwns = pkgOwnsBin(right.name, right.pkgName)
  if (leftOwns && !rightOwns) return 1
  if (!leftOwns && rightOwns) return -1
  if (left.pkgName !== right.pkgName) return left.pkgName.localeCompare(right.pkgName) // it's pointless to compare versions of 2 different package
  return semver.compare(left.pkgVersion, right.pkgVersion)
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
  let manifest = opts.allowExoticManifests
    ? (await safeReadProjectManifestOnly(target) as DependencyManifest)
    : await safeReadPkgJson(target)

  if (manifest == null) {
    manifest = await readLinkedPublishManifest(target) as DependencyManifest
  }

  if (manifest == null) {
    // There's a directory in node_modules without package.json: ${target}.
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

/**
 * Returns the manifest of the project whose `publishConfig.directory` the
 * dependency link `target` points to, or `null` when there is no such project
 * or `target` does not exist. The link is matched both by its own target and
 * by its real path, so a publish directory that is itself a symlink matches.
 * Other filesystem errors and unreadable manifests are thrown.
 */
async function readLinkedPublishManifest (target: string): Promise<ProjectManifest | null> {
  const candidates = new Set<string>()
  for (const resolve of [readLinkTarget, fs.realpath]) {
    try {
      // eslint-disable-next-line no-await-in-loop -- two cheap lookups whose errors are filtered one at a time
      candidates.add(await resolve(target))
    } catch (err: unknown) {
      const code = (err as NodeJS.ErrnoException).code
      if (code !== 'ENOENT' && code !== 'EINVAL') throw err
    }
  }
  for (const candidate of candidates) {
    // eslint-disable-next-line no-await-in-loop -- stops at the first candidate that has a publish manifest
    const manifest = await safeReadParentPublishManifest(candidate)
    if (manifest != null) return manifest
  }
  return null
}

async function readLinkTarget (link: string): Promise<string> {
  return path.resolve(path.dirname(link), await fs.readlink(link))
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
      nodeExecPath = path.join(nodeRuntimeBinDir(nodeDir), IS_WINDOWS ? 'node.exe' : 'node')
    }
  }
  return cmds.map((cmd) => ({
    ...cmd,
    pkgDir,
    pkgName: manifest.name,
    pkgVersion: manifest.version,
    makePowerShellShim: manifest.name !== 'pnpm',
    nodeExecPath,
  }))
}

function runtimeHasNodeDownloaded (runtime: EngineDependency | EngineDependency[]): boolean {
  if (!Array.isArray(runtime)) {
    return runtime.name === 'node' && runtime.onFail === 'download'
  }
  return runtime.find(({ name }) => name === 'node')?.onFail === 'download'
}

/**
 * A package's own bins are on PATH while its lifecycle scripts run, and those
 * scripts may be what creates a missing target. The `node` package's
 * preinstall calls `node` to download bin/node, which must not resolve to a
 * shim of bin/node itself. So a package's own bin is not linked into its own
 * .bin while the target is missing, and neither is any bin of a pass with
 * `holdBackMissingTargets`. Other bins get the shim (see cmd-shim).
 *
 * @returns `true` when the bin is held back: its target is missing, and any
 * shim an earlier install left for it, with its Windows siblings, is removed.
 * `false` when the bin should be linked.
 * @throws When probing the target or removing the shim fails for a reason
 * other than a missing path.
 */
async function removeBinIfTargetAwaited (cmd: CommandInfo, binsDir: string, opts: LinkBinOptions): Promise<boolean> {
  if (!opts.holdBackMissingTargets && !isOwnBinsDir(cmd.pkgDir, binsDir)) return false
  if (!await isBinTargetMissing(cmd.path)) return false
  await removeBin(path.join(binsDir, cmd.name))
  return true
}

async function removeBin (binPath: string): Promise<void> {
  await Promise.all([
    rimraf(binPath),
    ...(IS_WINDOWS ? ['.cmd', '.ps1', getExeExtension()].map(async (ext) => rimraf(`${binPath}${ext}`)) : []),
  ])
}

function isOwnBinsDir (pkgDir: string, binsDir: string): boolean {
  return path.resolve(pkgDir, 'node_modules', '.bin') === path.resolve(binsDir)
}

// A target without an extension is run directly, and Windows then finds its .exe.
async function isBinTargetMissing (target: string): Promise<boolean> {
  if (!await isMissing(target)) return false
  return !IS_WINDOWS || path.extname(target) !== '' || isMissing(`${target}${getExeExtension()}`)
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
