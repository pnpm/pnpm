import PnpmError from '@pnpm/error'
import binify, { Command } from '@pnpm/package-bins'
import readModulesDir from '@pnpm/read-modules-dir'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { DependencyManifest } from '@pnpm/types'
import Module = require('module')
import cmdShim = require('@zkochan/cmd-shim')
import isSubdir = require('is-subdir')
import isWindows = require('is-windows')
import fs = require('mz/fs')
import normalizePath = require('normalize-path')
import pSettle = require('p-settle')
import path = require('path')
import R = require('ramda')

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
  const allCmds = R.unnest(
    (await Promise.all(
      allDeps
        .map((depName) => path.resolve(modulesDir, depName))
        .filter((depDir) => !isSubdir(depDir, binsDir)) // Don't link own bins
        .map((depDir) => normalizePath(depDir))
        .map(getPackageBins.bind(null, pkgBinOpts))
    ))
      .filter((cmds: Command[]) => cmds.length)
  )

  return linkBins(allCmds, binsDir, opts)
}

export async function linkBinsOfPackages (
  pkgs: Array<{
    manifest: DependencyManifest
    location: string
  }>,
  binsTarget: string,
  opts: {
    warn: WarnFunction
  }
): Promise<string[]> {
  if (!pkgs.length) return []

  const allCmds = R.unnest(
    (await Promise.all(
      pkgs
        .map((pkg) => getPackageBinsFromManifest(pkg.manifest, pkg.location))
    ))
      .filter((cmds: Command[]) => cmds.length)
  )

  return linkBins(allCmds, binsTarget, opts)
}

async function linkBins (
  allCmds: Array<Command & {
    ownName: boolean
    pkgName: string
  }>,
  binsDir: string,
  opts: {
    warn: WarnFunction
  }
): Promise<string[]> {
  if (!allCmds.length) return [] as string[]

  await fs.mkdir(binsDir, { recursive: true })

  const [cmdsWithOwnName, cmdsWithOtherNames] = R.partition(({ ownName }) => ownName, allCmds)

  const results1 = await pSettle(cmdsWithOwnName.map((cmd: Command) => linkBin(cmd, binsDir)))

  const usedNames = R.fromPairs(cmdsWithOwnName.map((cmd) => [cmd.name, cmd.name] as R.KeyValuePair<string, string>))
  const results2 = await pSettle(cmdsWithOtherNames.map((cmd: Command & {pkgName: string}) => {
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
  target: string
) {
  const manifest = opts.allowExoticManifests
    ? (await safeReadProjectManifestOnly(target) as DependencyManifest) : await safeReadPkgJson(target)

  if (!manifest) {
    // There's a directory in node_modules without package.json: ${target}.
    // This used to be a warning but it didn't really cause any issues.
    return []
  }

  if (R.isEmpty(manifest.bin) && !await isFromModules(target)) {
    opts.warn(`Package in ${target} must have a non-empty bin field to get bin linked.`, 'EMPTY_BIN')
  }

  if (typeof manifest.bin === 'string' && !manifest.name) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package in ${target} must have a name to get bin linked.`)
  }

  return getPackageBinsFromManifest(manifest, target)
}

async function getPackageBinsFromManifest (manifest: DependencyManifest, pkgDir: string) {
  const cmds = await binify(manifest, pkgDir)
  return cmds.map((cmd) => ({
    ...cmd,
    ownName: cmd.name === manifest.name,
    pkgName: manifest.name,
  }))
}

async function linkBin (cmd: Command, binsDir: string) {
  const externalBinPath = path.join(binsDir, cmd.name)

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    await fs.chmod(cmd.path, 0o755)
  }
  let nodePath = await getBinNodePaths(cmd.path)
  const binsParentDir = path.dirname(binsDir)
  if (path.relative(cmd.path, binsParentDir) !== '') {
    nodePath = R.union(nodePath, await getBinNodePaths(binsParentDir))
  }
  return cmdShim(cmd.path, externalBinPath, {
    createPwshFile: POWER_SHELL_IS_SUPPORTED,
    nodePath,
  })
}

async function getBinNodePaths (target: string): Promise<string[]> {
  const targetRealPath = await fs.realpath(target)

  return R.union(
    Module._nodeModulePaths(targetRealPath),
    Module._nodeModulePaths(target)
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
