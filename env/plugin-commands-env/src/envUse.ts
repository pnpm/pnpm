import { promises as fs } from 'fs'
import util from 'util'
import gfs from 'graceful-fs'
import path from 'path'
import normalizePath from 'normalize-path'
import { PnpmError } from '@pnpm/error'
import { getBinNodePaths, getPackageBins, preferDirectCmds } from '@pnpm/link-bins'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { getExtraNodePaths } from '@pnpm/get-context'
import { logger } from '@pnpm/logger'
import type { Command } from '@pnpm/package-bins'
import cmdShim from '@zkochan/cmd-shim'
import isWindows from 'is-windows'
import symlinkDir from 'symlink-dir'
import unnest from 'ramda/src/unnest'
import { type NvmNodeCommandOptions } from './node'
import { CURRENT_NODE_DIRNAME, getNodeExecPathInBinDir, getNodeExecPathInNodeDir } from './utils'
import { downloadNodeVersion } from './downloadNodeVersion'

export async function envUse (opts: NvmNodeCommandOptions, params: string[]) {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }
  const nodeInfo = await downloadNodeVersion(opts, params[0])
  if (!nodeInfo) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${params[0]}`)
  }
  const { nodeDir, nodeVersion } = nodeInfo
  const src = getNodeExecPathInNodeDir(nodeDir)
  const dest = getNodeExecPathInBinDir(opts.bin)
  await symlinkDir(nodeDir, path.join(opts.pnpmHomeDir, CURRENT_NODE_DIRNAME))
  try {
    gfs.unlinkSync(dest)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) throw err
  }
  await symlinkOrHardLink(src, dest)
  try {
    let npmDir = nodeDir
    if (process.platform !== 'win32') {
      npmDir = path.join(npmDir, 'lib')
    }
    npmDir = path.join(npmDir, 'node_modules/npm')
    if (opts.configDir) {
      // We want the global npm settings to persist when Node.js or/and npm is changed to a different version,
      // so we tell npm to read the global config from centralized place that is outside of npm's directory.
      await fs.writeFile(path.join(npmDir, 'npmrc'), `globalconfig=${path.join(opts.configDir, 'npmrc')}`, 'utf-8')
    }
    const npmBinDir = path.join(npmDir, 'bin')
    const cmdShimOpts = { createPwshFile: false }

    type CommandInfo = Command & {
      ownName: boolean
      pkgName: string
      makePowerShellShim: boolean
      nodeExecPath?: string
    }
    const globalDepDir = path.join(opts.rootProjectManifestDir, 'node_modules')
    const allDeps = await readModulesDir(globalDepDir) ?? []
    const directDependencies = new Set(Object.keys((opts.rootProjectManifest?.dependencies ?? {})))
    const binWarn = (prefix: string, message: string) => {
      logger.info({ message, prefix })
    }
    const allCmds: CommandInfo[] = unnest(
      (await Promise.all(
        allDeps.map((alias) => ({
          depDir: path.resolve(globalDepDir, alias),
          isDirectDependency: directDependencies?.has(alias),
          nodeExecPath: src,
        }))
          .map(async ({ depDir, isDirectDependency, nodeExecPath }) => {
            const target = normalizePath(depDir)
            const cmds = await getPackageBins({ allowExoticManifests: false, warn: binWarn.bind(null, opts.rootProjectManifestDir) }, target, nodeExecPath)
            return cmds.map((cmd) => ({ ...cmd, isDirectDependency }))
          })
      ))
        .filter((cmds: Command[]) => cmds.length)
    )
    const cmdsToLink = preferDirectCmds(allCmds)

    await Promise.all(cmdsToLink.map(async (cmd) => {
      const nodePath: string[] = []
      const virtualStoreDir = path.join(opts.rootProjectManifestDir, opts.virtualStoreDir ?? '')
      const extraNodePaths = getExtraNodePaths({ extendNodePath: opts.extendNodePath, nodeLinker: opts.nodeLinker ?? 'isolated', hoistPattern: opts.hoistPattern, virtualStoreDir })
      for (const modulesPath of await getBinNodePaths(cmd.path)) {
        if (extraNodePaths.includes(modulesPath)) break
        nodePath.push(modulesPath)
      }
      nodePath.push(...extraNodePaths)
      const externalBinPath = path.join(opts.bin, cmd.name)
      await cmdShim(cmd.path, externalBinPath, {
        createPwshFile: cmd.makePowerShellShim,
        nodePath,
        nodeExecPath: cmd.nodeExecPath,
      })
    }))

    await cmdShim(path.join(npmBinDir, 'npm-cli.js'), path.join(opts.bin, 'npm'), cmdShimOpts)
    await cmdShim(path.join(npmBinDir, 'npx-cli.js'), path.join(opts.bin, 'npx'), cmdShimOpts)
  } catch {
    // ignore
  }
  return `Node.js ${nodeVersion as string} was activated
${dest} -> ${src}`
}

// On Windows, symlinks only work with developer mode enabled
// or with admin permissions. So it is better to use hard links on Windows.
async function symlinkOrHardLink (existingPath: string, newPath: string) {
  if (isWindows()) {
    return fs.link(existingPath, newPath)
  }
  return fs.symlink(existingPath, newPath, 'file')
}
