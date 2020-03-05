import PnpmError from '@pnpm/error'
import binify, { Command } from '@pnpm/package-bins'
import readModulesDir from '@pnpm/read-modules-dir'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import { DependencyManifest } from '@pnpm/types'
import cmdShim = require('@zkochan/cmd-shim')
import isSubdir = require('is-subdir')
import isWindows = require('is-windows')
import makeDir = require('make-dir')
import Module = require('module')
import fs = require('mz/fs')
import normalizePath = require('normalize-path')
import pSettle = require('p-settle')
import path = require('path')
import R = require('ramda')

const IS_WINDOWS = isWindows()
const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS
const POWER_SHELL_IS_SUPPORTED = IS_WINDOWS

export default async (
  modules: string,
  binPath: string,
  opts: {
    allowExoticManifests?: boolean,
    warn: (msg: string) => void,
  },
) => {
  const pkgDirs = await readModulesDir(modules)
  // If the modules dir does not exist, do nothing
  if (pkgDirs === null) return
  const pkgBinOpts = {
    allowExoticManifests: false,
    ...opts,
  }
  const allCmds = R.unnest(
    (await Promise.all(
      pkgDirs
        .map((dir) => path.resolve(modules, dir))
        .filter((dir) => !isSubdir(dir, binPath)) // Don't link own bins
        .map((dir) => normalizePath(dir))
        .map((target: string) => getPackageBins(target, pkgBinOpts)),
    ))
    .filter((cmds: Command[]) => cmds.length),
  )

  return linkBins(allCmds, binPath, opts)
}

export async function linkBinsOfPackages (
  pkgs: Array<{
    manifest: DependencyManifest,
    location: string,
  }>,
  binsTarget: string,
  opts: {
    warn: (msg: string) => void,
  },
) {
  if (!pkgs.length) return

  const allCmds = R.unnest(
    (await Promise.all(
      pkgs
        .map((pkg) => getPackageBinsFromPackageJson(pkg.manifest, pkg.location)),
    ))
    .filter((cmds: Command[]) => cmds.length),
  )

  return linkBins(allCmds, binsTarget, opts)
}

async function linkBins (
  allCmds: Array<Command & {
    ownName: boolean,
    pkgName: string,
  }>,
  binPath: string,
  opts: {
    warn: (msg: string) => void,
  },
) {
  if (!allCmds.length) return

  await makeDir(binPath)

  const [cmdsWithOwnName, cmdsWithOtherNames] = R.partition((cmd) => cmd.ownName, allCmds)

  const results1 = await pSettle(cmdsWithOwnName.map((cmd: Command) => linkBin(cmd, binPath)))

  const usedNames = R.fromPairs(cmdsWithOwnName.map((cmd) => [cmd.name, cmd.name] as R.KeyValuePair<string, string>))
  const results2 = await pSettle(cmdsWithOtherNames.map((cmd: Command & {pkgName: string}) => {
    if (usedNames[cmd.name]) {
      opts.warn(`Cannot link bin "${cmd.name}" of "${cmd.pkgName}" to "${binPath}". A package called "${usedNames[cmd.name]}" already has its bin linked.`)
      return Promise.resolve(undefined)
    }
    usedNames[cmd.name] = cmd.pkgName
    return linkBin(cmd, binPath)
  }))

  // We want to create all commands that we can create before throwing an exception
  for (const result of [...results1, ...results2]) {
    if (result.isRejected) {
      throw result.reason
    }
  }
}

async function isFromModules (filename: string) {
  const real = await fs.realpath(filename)
  return normalizePath(real).includes('/node_modules/')
}

async function getPackageBins (
  target: string,
  opts: {
    allowExoticManifests: boolean,
    warn: (msg: string) => void,
  },
) {
  const pkg = opts.allowExoticManifests ? await safeReadProjectManifestOnly(target) : await safeReadPkg(target)

  if (!pkg) {
    // There's a directory in node_modules without package.json: ${target}.
    // This used to be a warning but it didn't really cause any issues.
    return []
  }

  if (R.isEmpty(pkg.bin) && !await isFromModules(target)) {
    opts.warn(`Package in ${target} must have a non-empty bin field to get bin linked.`)
  }

  if (typeof pkg.bin === 'string' && !pkg.name) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package in ${target} must have a name to get bin linked.`)
  }

  return getPackageBinsFromPackageJson(pkg, target)
}

async function getPackageBinsFromPackageJson (pkgJson: DependencyManifest, pkgPath: string) {
  const cmds = await binify(pkgJson, pkgPath)
  return cmds.map((cmd) => ({
    ...cmd,
    ownName: cmd.name === pkgJson.name,
    pkgName: pkgJson.name,
  }))
}

async function linkBin (cmd: Command, binPath: string) {
  const externalBinPath = path.join(binPath, cmd.name)

  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    await fs.chmod(cmd.path, 0o755)
  }
  const nodePath = await getBinNodePaths(cmd.path)
  return cmdShim(cmd.path, externalBinPath, {
    createPwshFile: POWER_SHELL_IS_SUPPORTED,
    nodePath,
  })
}

async function getBinNodePaths (target: string): Promise<string[]> {
  const targetRealPath = await fs.realpath(target)

  return R.union(
    Module._nodeModulePaths(targetRealPath),
    Module._nodeModulePaths(target),
  )
}

async function safeReadPkg (pkgPath: string): Promise<DependencyManifest | null> {
  try {
    return await readPackageJsonFromDir(pkgPath) as DependencyManifest
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      return null
    }
    throw err
  }
}

async function safeReadProjectManifestOnly (projectDir: string) {
  try {
    return await readProjectManifestOnly(projectDir) as DependencyManifest
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') {
      return null
    }
    throw err
  }
}
