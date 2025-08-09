import { promises as fs } from 'fs'
import util from 'util'
import gfs from 'graceful-fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import semver from 'semver'
import cmdShim from '@zkochan/cmd-shim'
import isWindows from 'is-windows'
import symlinkDir from 'symlink-dir'
import { type NvmNodeCommandOptions } from './node'
import { CURRENT_NODE_DIRNAME, getNodeExecPathInBinDir, getNodeExecPathInNodeDir } from './utils'
import { downloadNodeVersion } from './downloadNodeVersion'
import { getNodeRuntime, updateNodeRuntimeVersion } from '@pnpm/manifest-utils'

export async function envUse (opts: NvmNodeCommandOptions, params: string[]): Promise<string> {
  let [nodeVersion] = params
  if (!nodeVersion) {
    throw new PnpmError('ENV_USE_NO_PARAMS', '`pnpm env use` requires a Node.js version specifier')
  }
  const prefix = opts.rootProjectManifestDir
  let overwriteVersion
  if (!opts.global) {
    if (opts.useNodeVersion && semver.satisfies(opts.useNodeVersion, nodeVersion)) {
      // Pass along the specific useNodeVersion if already within the given range,
      // to avoid unnecessary slow API calls inside getNodeVersion()
      nodeVersion = opts.useNodeVersion

    } else if (!opts.useNodeVersion && opts.rootProjectManifest?.devEngines) {
      const { version, onFail } = getNodeRuntime(opts.rootProjectManifest.devEngines) ?? {}
      if (version) {
        const message = `"Node.js version "${nodeVersion}" is incompatible with "devEngines.runtime.version: ${version}"`
        switch (onFail) {
          case 'download':
            overwriteVersion = version
            break
          case 'ignore':
            return ''
          case 'warn':
            logger.warn({ message, prefix })
            break
          case 'error':
          default: throw new PnpmError('INVALID_NODE_VERSION', message)
        }
      }
    }
  }
  const nodeInfo = await downloadNodeVersion(opts, nodeVersion)
  if (!nodeInfo) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching "${nodeVersion}"`)
  }
  const {nodeDir} = nodeInfo;
  nodeVersion = nodeInfo.nodeVersion
  if (overwriteVersion) {
    const range = `^${semver.major(nodeVersion)}`
    await updateNodeRuntimeVersion(prefix, opts.rootProjectManifest, range)

    const message = `"devEngines.runtime.version": "${overwriteVersion}" was modified to "${range}"`
    logger.info({ message, prefix })
  }
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
