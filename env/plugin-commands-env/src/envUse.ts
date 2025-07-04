import { promises as fs } from 'fs'
import util from 'util'
import gfs from 'graceful-fs'
import path from 'path'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { satisfies } from 'semver'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import cmdShim from '@zkochan/cmd-shim'
import isWindows from 'is-windows'
import symlinkDir from 'symlink-dir'
import { type NvmNodeCommandOptions } from './node'
import { CURRENT_NODE_DIRNAME, getNodeExecPathInBinDir, getNodeExecPathInNodeDir } from './utils'
import { getNodeVersion, downloadNodeVersion } from './downloadNodeVersion'

export async function envUse (opts: NvmNodeCommandOptions, params: string[]): Promise<string> {
  if (!opts.global) {
    const {runtime} = opts.rootProjectManifest.devEngines ?? {}
    const runtimeNodeVersion = runtime?.name === 'node' ? runtime?.version : undefined

    const version = params.at(-1) ?? opts.useNodeVersion ?? runtimeNodeVersion ?? 'latest'
    const {nodeVersion} = opts.rootProjectManifest.pnpm?.executionEnv ?? await getNodeVersion(opts, version)

    if (!opts.useNodeVersion || params.length) {
      await updateWorkspaceManifest(opts.workspaceDir, { useNodeVersion: nodeVersion })
      opts.useNodeVersion = nodeVersion
    }
    if (runtimeNodeVersion && !satisfies(opts.useNodeVersion, runtimeNodeVersion)) {
      const message = `"useNodeVersion: ${opts.useNodeVersion}" is incompatible with "devEngines.runtime.version: ${runtimeNodeVersion}"`
      if (opts.engineStrict) throw new PnpmError('INVALID_NODE_VERSION', message)
      else logger.warn({ message, prefix: opts.workspaceDir })
    }
    params[0] ??= nodeVersion
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
async function symlinkOrHardLink (existingPath: string, newPath: string): Promise<void> {
  if (isWindows()) {
    return fs.link(existingPath, newPath)
  }
  return fs.symlink(existingPath, newPath, 'file')
}
