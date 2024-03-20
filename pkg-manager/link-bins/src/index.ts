import '@total-typescript/ts-reset'
import path from 'node:path'
import { promises as fs, existsSync } from 'node:fs'

import Module from 'module'
import pSettle from 'p-settle'
import isSubdir from 'is-subdir'
import isWindows from 'is-windows'
import rimraf from '@zkochan/rimraf'
import symlinkDir from 'symlink-dir'
import unnest from 'ramda/src/unnest'
import cmdShim from '@zkochan/cmd-shim'
import isEmpty from 'ramda/src/isEmpty'
import { type KeyValuePair } from 'ramda'
import normalizePath from 'normalize-path'
import fixBin from 'bin-links/lib/fix-bin'
import partition from 'ramda/src/partition'

import { PnpmError } from '@pnpm/error'
import { logger, globalWarn } from '@pnpm/logger'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { BundledManifest } from '@pnpm/store-controller-types'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { type DependencyManifest, type ProjectManifest } from '@pnpm/types'
import { type Command, getBinsFromPackageManifest } from '@pnpm/package-bins'

const binsConflictLogger = logger('bins-conflict')
const IS_WINDOWS = isWindows()
const EXECUTABLE_SHEBANG_SUPPORTED = !IS_WINDOWS
const POWER_SHELL_IS_SUPPORTED = IS_WINDOWS

export type WarningCode = 'BINARIES_CONFLICT' | 'EMPTY_BIN'

export type WarnFunction = (msg: string, code: WarningCode) => void

export async function linkBins(
  modulesDir: string | undefined,
  binsDir: string | undefined,
  opts: LinkBinOptions & {
    allowExoticManifests?: boolean | undefined
    nodeExecPathByAlias?: Record<string, string> | undefined
    projectManifest?: ProjectManifest | undefined
    warn: WarnFunction
  }
): Promise<string[]> {
  if (typeof modulesDir === 'undefined') {
    return []
  }

  const allDeps = await readModulesDir(modulesDir)
  // If the modules dir does not exist, do nothing

  if (allDeps === null) {
    return []
  }

  return linkBinsOfPkgsByAliases(allDeps, binsDir, {
    ...opts,
    modulesDir,
  })
}

export async function linkBinsOfPkgsByAliases(
  depsAliases: string[],
  binsDir: string | undefined,
  opts: LinkBinOptions & {
    modulesDir: string
    allowExoticManifests?: boolean | undefined
    nodeExecPathByAlias?: Record<string, string> | undefined
    projectManifest?: ProjectManifest | undefined
    warn: WarnFunction
  }
): Promise<string[]> {
  const pkgBinOpts = {
    allowExoticManifests: false,
    ...opts,
  }

  const directDependencies =
    opts.projectManifest == null
      ? undefined
      : new Set(
        Object.keys(getAllDependenciesFromManifest(opts.projectManifest))
      )

  const allCmds = unnest(
    (
      await Promise.all(
        depsAliases
          .map((alias: string): {
            depDir: string;
            isDirectDependency: boolean | undefined;
            nodeExecPath: string | undefined;
          } => {
            return {
              depDir: path.resolve(opts.modulesDir, alias),
              isDirectDependency: directDependencies?.has(alias),
              nodeExecPath: opts.nodeExecPathByAlias?.[alias],
            };
          })
          .filter(({ depDir }): boolean => !isSubdir(depDir, binsDir ?? '')) // Don't link own bins
          .map(async ({ depDir, isDirectDependency, nodeExecPath }) => {
            const target = normalizePath(depDir)

            const cmds = await getPackageBins(pkgBinOpts, target, nodeExecPath)

            return cmds.map((cmd: CommandInfo) => {
              return { ...cmd, isDirectDependency };
            })
          })
      )
    ).filter((cmds: Command[]): boolean => {
      return cmds.length > 0;
    })
  )

  const cmdsToLink =
    directDependencies != null ? preferDirectCmds(allCmds) : allCmds
  return _linkBins(cmdsToLink, binsDir, opts)
}

function preferDirectCmds(
  allCmds: Array<CommandInfo & { isDirectDependency?: boolean }>
) {
  const [directCmds, hoistedCmds] = partition(
    (cmd) => cmd.isDirectDependency === true,
    allCmds
  )
  const usedDirectCmds = new Set(directCmds.map((directCmd) => directCmd.name))
  return [
    ...directCmds,
    ...hoistedCmds.filter(({ name }): boolean => {
      return !usedDirectCmds.has(name);
    }),
  ]
}

export async function linkBinsOfPackages(
  pkgs: Array<{
    manifest: ProjectManifest | BundledManifest
    nodeExecPath?: string | undefined
    location: string
  }>,
  binsTarget: string | undefined,
  opts: LinkBinOptions = {}
): Promise<string[]> {
  if (pkgs.length === 0) {
    return []
  }

  const allCmds = unnest(
    (
      await Promise.all(
        pkgs.map(async (pkg): Promise<CommandInfo[]> => {
          return getPackageBinsFromManifest(
            pkg.manifest,
            pkg.location,
            pkg.nodeExecPath
          );
        }
        )
      )
    ).filter((cmds: Command[]): boolean => {
      return cmds.length > 0;
    })
  )

  return _linkBins(allCmds, binsTarget, opts)
}

type CommandInfo = Command & {
  ownName: boolean
  pkgName?: string | undefined
  makePowerShellShim: boolean
  nodeExecPath?: string | undefined
}

async function _linkBins(
  allCmds: CommandInfo[],
  binsDir: string | undefined,
  opts: LinkBinOptions
): Promise<string[]> {
  if (typeof binsDir === 'undefined' || allCmds.length === 0) {
    return []
  }

  await fs.mkdir(binsDir, { recursive: true })

  const [cmdsWithOwnName, cmdsWithOtherNames] = partition(
    ({ ownName }) => ownName,
    allCmds
  )

  const results1 = await pSettle(
    cmdsWithOwnName.map(async (cmd): Promise<void> => {
      return linkBin(cmd, binsDir, opts);
    })
  )

  const usedNames = Object.fromEntries(
    cmdsWithOwnName.map(
      (cmd: CommandInfo): KeyValuePair<string, string> => {
        return [cmd.name, cmd.name] as KeyValuePair<string, string>;
      }
    )
  )
  const results2 = await pSettle(
    cmdsWithOtherNames.map(async (cmd: CommandInfo): Promise<void> => {
      if (usedNames[cmd.name]) {
        binsConflictLogger.debug({
          binaryName: cmd.name,
          binsDir,
          linkedPkgName: usedNames[cmd.name],
          skippedPkgName: cmd.pkgName,
        })

        return Promise.resolve(undefined)
      }

      usedNames[cmd.name] = cmd.pkgName ?? '';

      return linkBin(cmd, binsDir, opts)
    })
  )

  // We want to create all commands that we can create before throwing an exception
  for (const result of [...results1, ...results2]) {
    if (result.isRejected) {
      throw result.reason
    }
  }

  return allCmds.map((cmd) => cmd.pkgName).filter(Boolean)
}

async function isFromModules(filename: string): Promise<boolean> {
  const real = await fs.realpath(filename)

  return normalizePath(real).includes('/node_modules/')
}

async function getPackageBins(
  opts: {
    allowExoticManifests: boolean
    warn: WarnFunction
  },
  target: string,
  nodeExecPath?: string | undefined
): Promise<CommandInfo[]> {
  const manifest = opts.allowExoticManifests
    ? ((await safeReadProjectManifestOnly(target)) as DependencyManifest)
    : await safeReadPkgJson(target)

  if (manifest == null) {
    // There's a directory in node_modules without package.json: ${target}.
    // This used to be a warning but it didn't really cause any issues.
    return []
  }

  if (isEmpty(manifest.bin) && !(await isFromModules(target))) {
    opts.warn(
      `Package in ${target} must have a non-empty bin field to get bin linked.`,
      'EMPTY_BIN'
    )
  }

  if (typeof manifest.bin === 'string' && !manifest.name) {
    throw new PnpmError(
      'INVALID_PACKAGE_NAME',
      `Package in ${target} must have a name to get bin linked.`
    )
  }

  return getPackageBinsFromManifest(manifest, target, nodeExecPath)
}

async function getPackageBinsFromManifest(
  manifest: ProjectManifest | BundledManifest,
  pkgDir: string,
  nodeExecPath?: string | undefined
): Promise<CommandInfo[]> {
  const cmds = await getBinsFromPackageManifest(manifest, pkgDir)

  return cmds.map((cmd) => {
    return {
      ...cmd,
      ownName: cmd.name === manifest.name,
      pkgName: manifest.name,
      makePowerShellShim: POWER_SHELL_IS_SUPPORTED && manifest.name !== 'pnpm',
      nodeExecPath,
    };
  })
}

export interface LinkBinOptions {
  extraNodePaths?: string[] | undefined
  preferSymlinkedExecutables?: boolean | undefined
}

async function linkBin(
  cmd: CommandInfo,
  binsDir: string,
  opts?: LinkBinOptions | undefined
) {
  const externalBinPath = path.join(binsDir, cmd.name)

  if (IS_WINDOWS) {
    const exePath = path.join(binsDir, `${cmd.name}${getExeExtension()}`)

    if (existsSync(exePath)) {
      globalWarn(
        `The target bin directory already contains an exe called ${cmd.name}, so removing ${exePath}`
      )

      await rimraf(exePath)
    }
  }

  if (
    opts?.preferSymlinkedExecutables &&
    !IS_WINDOWS &&
    cmd.nodeExecPath == null
  ) {
    try {
      await symlinkDir(cmd.path, externalBinPath)
      await fixBin(cmd.path, 0o7_5_5)
    } catch (err: unknown) {
      // @ts-ignore
      if (err.code !== 'ENOENT') {
        throw err
      }

      globalWarn(
        // @ts-ignore
        `Failed to create bin at ${externalBinPath}. ${err.message}`
      )
    }
    return
  }

  try {
    let nodePath: string[] | undefined

    if (opts?.extraNodePaths?.length) {
      nodePath = []

      for (const modulesPath of await getBinNodePaths(cmd.path)) {
        if (opts.extraNodePaths.includes(modulesPath)) {
          break
        }

        nodePath.push(modulesPath)
      }

      nodePath.push(...opts.extraNodePaths)
    }

    await cmdShim(cmd.path, externalBinPath, {
      createPwshFile: cmd.makePowerShellShim,
      nodePath,
      nodeExecPath: cmd.nodeExecPath,
    })
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code !== 'ENOENT') {
      throw err
    }

    globalWarn(
      // @ts-ignore
      `Failed to create bin at ${externalBinPath}. ${err.message}`
    )

    return
  }

  // ensure that bin are executable and not containing
  // windows line-endings(CRLF) on the hashbang line
  if (EXECUTABLE_SHEBANG_SUPPORTED) {
    await fixBin(cmd.path, 0o7_5_5)
  }
}

function getExeExtension(): string {
  let cmdExtension

  if (process.env.PATHEXT) {
    cmdExtension = process.env.PATHEXT.split(path.delimiter).find(
      (ext) => ext.toUpperCase() === '.EXE'
    )
  }

  return cmdExtension ?? '.exe'
}

async function getBinNodePaths(target: string): Promise<string[]> {
  const targetDir = path.dirname(target)

  try {
    const targetRealPath = await fs.realpath(targetDir)
    // @ts-expect-error
    return Module._nodeModulePaths(targetRealPath)
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code !== 'ENOENT') {
      throw err
    }
    // @ts-expect-error
    return Module._nodeModulePaths(targetDir)
  }
}

async function safeReadPkgJson(
  pkgDir: string
): Promise<DependencyManifest | null> {
  try {
    return (await readPackageJsonFromDir(pkgDir)) as DependencyManifest
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      return null
    }
    throw err
  }
}
