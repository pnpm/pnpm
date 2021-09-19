import { promises as fs } from 'fs'
import Module from 'module'
import path from 'path'
import PnpmError from '@pnpm/error'
import binify, { Command } from '@pnpm/package-bins'
import readModulesDir from '@pnpm/read-modules-dir'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { DependencyManifest } from '@pnpm/types'
import cmdShim from '@zkochan/cmd-shim'
import isSubdir from 'is-subdir'
import isWindows from 'is-windows'
import normalizePath from 'normalize-path'
import pSettle from 'p-settle'
import { KeyValuePair } from 'ramda'
import fromPairs from 'ramda/src/fromPairs'
import isEmpty from 'ramda/src/isEmpty'
import union from 'ramda/src/union'
import unnest from 'ramda/src/unnest'
import partition from 'ramda/src/partition'

const IS_WINDOWS = isWindows()
const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS
const POWER_SHELL_IS_SUPPORTED = IS_WINDOWS

export type WarningCode = 'BINARIES_CONFLICT' | 'EMPTY_BIN'

export type WarnFunction = (msg: string, code: WarningCode) => void

export default async (
  modulesDir: string,
  binsDir: string,
  opts: {
    allowExoticManifests?: boolean
    nodeExecPathByAlias?: Record<string, string>
    warn: WarnFunction
  }
): Promise<string[]> => {
  const allDeps = await readModulesDir(modulesDir)
  // If the modules dir does not exist, do nothing
  if (allDeps === null) return []
  const pkgBinOpts = {
    allowExoticManifests: false,
    ...opts,
  }
  const allCmds = unnest(
    (await Promise.all(
      allDeps
        .map((alias) => ({
          depDir: path.resolve(modulesDir, alias),
          nodeExecPath: opts.nodeExecPathByAlias?.[alias],
        }))
        .filter(({ depDir }) => !isSubdir(depDir, binsDir)) // Don't link own bins
        .map(({ depDir, nodeExecPath }) => {
          const target = normalizePath(depDir)
          return getPackageBins(pkgBinOpts, target, nodeExecPath)
        })
    ))
      .filter((cmds: Command[]) => cmds.length)
  )

  return linkBins(allCmds, binsDir, opts)
}

export async function linkBinsOfPackages (
  pkgs: Array<{
    manifest: DependencyManifest
    nodeExecPath?: string
    location: string
  }>,
  binsTarget: string,
  opts: {
    warn: WarnFunction
  }
): Promise<string[]> {
  if (pkgs.length === 0) return []

  const allCmds = unnest(
    (await Promise.all(
      pkgs
        .map(async (pkg) => getPackageBinsFromManifest(pkg.manifest, pkg.location, pkg.nodeExecPath))
    ))
      .filter((cmds: Command[]) => cmds.length)
  )

  return linkBins(allCmds, binsTarget, opts)
}

type CommandInfo = Command & {
  ownName: boolean
  pkgName: string
  makePowerShellShim: boolean
  nodeExecPath?: string
}

async function linkBins (
  allCmds: CommandInfo[],
  binsDir: string,
  opts: {
    warn: WarnFunction
  }
): Promise<string[]> {
  if (allCmds.length === 0) return [] as string[]

  await fs.mkdir(binsDir, { recursive: true })

  const [cmdsWithOwnName, cmdsWithOtherNames] = partition(({ ownName }) => ownName, allCmds)

  const results1 = await pSettle(cmdsWithOwnName.map(async (cmd) => linkBin(cmd, binsDir)))

  const usedNames = fromPairs(cmdsWithOwnName.map((cmd) => [cmd.name, cmd.name] as KeyValuePair<string, string>))
  const results2 = await pSettle(cmdsWithOtherNames.map(async (cmd) => {
    if (usedNames[cmd.name]) {
      opts.warn(`Cannot link binary '${cmd.name}' of '${cmd.pkgName}' to '${binsDir}': binary of '${usedNames[cmd.name]}' is already linked`, 'BINARIES_CONFLICT')
      return Promise.resolve(undefined)
    }
    usedNames[cmd.name] = cmd.pkgName
    return linkBin(cmd, binsDir)
  }))

  // We want to create all commands that we can create before throwing an exception
  for (const result of [...results1, ...results2]) {
    if (result.isRejected) {
      throw result.reason
    }
  }

  return allCmds.map(cmd => cmd.pkgName)
}

async function isFromModules (filename: string) {
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
  const cmds = await binify(manifest, pkgDir)
  return cmds.map((cmd) => ({
    ...cmd,
    ownName: cmd.name === manifest.name,
    pkgName: manifest.name,
    makePowerShellShim: POWER_SHELL_IS_SUPPORTED && manifest.name !== 'pnpm',
    nodeExecPath,
  }))
}

async function linkBin (cmd: CommandInfo, binsDir: string) {
  const externalBinPath = path.join(binsDir, cmd.name)

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    await fs.chmod(cmd.path, 0o755)
  }
  let nodePath = await getBinNodePaths(cmd.path)
  const binsParentDir = path.dirname(binsDir)
  if (path.relative(cmd.path, binsParentDir) !== '') {
    nodePath = union(nodePath, await getBinNodePaths(binsParentDir))
  }
  return cmdShim(cmd.path, externalBinPath, {
    createPwshFile: cmd.makePowerShellShim,
    nodePath,
    nodeExecPath: cmd.nodeExecPath,
  })
}

async function getBinNodePaths (target: string): Promise<string[]> {
  const targetDir = path.dirname(target)
  const targetRealPath = await fs.realpath(targetDir)

  return union(
    Module['_nodeModulePaths'](targetRealPath),
    Module['_nodeModulePaths'](targetDir)
  )
}

async function safeReadPkgJson (pkgDir: string): Promise<DependencyManifest | null> {
  try {
    return await readPackageJsonFromDir(pkgDir) as DependencyManifest
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      return null
    }
    throw err
  }
}
