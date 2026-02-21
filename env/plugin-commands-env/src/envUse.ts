import { existsSync, promises as fs } from 'fs'
import util from 'util'
import gfs from 'graceful-fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { add } from '@pnpm/plugin-commands-installation'
import cmdShim from '@zkochan/cmd-shim'
import isWindows from 'is-windows'
import symlinkDir from 'symlink-dir'
import { writeJsonFile } from 'write-json-file'
import { getNodeVersion } from './downloadNodeVersion.js'
import { type NvmNodeCommandOptions } from './node.js'
import { CURRENT_NODE_DIRNAME, getNodeExecPathInBinDir, getNodeExecPathInNodeDir } from './utils.js'

export async function envUse (opts: NvmNodeCommandOptions, params: string[]): Promise<string> {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }

  // 1. Resolve version specifier (lts, range, codename) → exact version
  const { nodeVersion } = await getNodeVersion(opts, params[0])
  if (!nodeVersion) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${params[0]}`)
  }

  // 2. Prepare the synthetic env project directory
  const envDir = path.join(opts.pnpmHomeDir, 'env')
  await fs.mkdir(envDir, { recursive: true })
  const manifestPath = path.join(envDir, 'package.json')
  if (!existsSync(manifestPath)) {
    await writeJsonFile(manifestPath, { name: 'pnpm-env', private: true, dependencies: {} })
  }

  // 3. Install node@runtime:{version} via pnpm's own install machinery.
  //    We pre-resolve to an exact version so add.handler doesn't need to resolve it.
  await add.handler({
    // Defaults for fields not in NvmNodeCommandOptions
    registries: { default: 'https://registry.npmjs.org/' },
    rawLocalConfig: {},
    argv: { original: [] },
    pnpmfile: [],
    // Forward the user's opts (network settings, store path, registry config, etc.)
    ...opts,
    // Override to install into the env project dir
    dir: envDir,
    bin: envDir,
    lockfileDir: envDir,
    rootProjectManifestDir: envDir,
    saveProd: true,
    saveDev: false,
    saveOptional: false,
    savePeer: false,
    symlink: true,
    workspaceDir: undefined,
    ignoreWorkspaceRootCheck: true,

  } as unknown as Parameters<typeof add.handler>[0], [`node@runtime:${nodeVersion}`])

  // 4. Find node executable inside the installed node_modules
  const nodeModulesNodeDir = path.join(envDir, 'node_modules', 'node')
  const src = getNodeExecPathInNodeDir(nodeModulesNodeDir)
  const dest = getNodeExecPathInBinDir(opts.bin)

  // 5. Update the nodejs_current symlink to point to the new node directory
  await symlinkDir(nodeModulesNodeDir, path.join(opts.pnpmHomeDir, CURRENT_NODE_DIRNAME))

  // 6. Link the node executable into the user's bin dir
  try {
    gfs.unlinkSync(dest)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) throw err
  }
  await symlinkOrHardLink(src, dest)

  // 7. Create npm/npx shims pointing into the newly installed node's npm
  try {
    let npmDir = nodeModulesNodeDir
    if (process.platform !== 'win32') {
      npmDir = path.join(npmDir, 'lib')
    }
    npmDir = path.join(npmDir, 'node_modules/npm')
    if (opts.configDir) {
      await fs.writeFile(path.join(npmDir, 'npmrc'), `globalconfig=${path.join(opts.configDir, 'npmrc')}`, 'utf-8')
    }
    const npmBinDir = path.join(npmDir, 'bin')
    const cmdShimOpts = { createPwshFile: false }
    await cmdShim(path.join(npmBinDir, 'npm-cli.js'), path.join(opts.bin, 'npm'), cmdShimOpts)
    await cmdShim(path.join(npmBinDir, 'npx-cli.js'), path.join(opts.bin, 'npx'), cmdShimOpts)
  } catch {
    // ignore — npm/npx shimming is best-effort
  }

  return `Node.js ${nodeVersion as string} was activated\n${dest} -> ${src}`
}

// On Windows, symlinks only work with developer mode enabled
// or with admin permissions. So it is better to use hard links on Windows.
async function symlinkOrHardLink (existingPath: string, newPath: string): Promise<void> {
  if (isWindows()) {
    return fs.link(existingPath, newPath)
  }
  return fs.symlink(existingPath, newPath, 'file')
}
