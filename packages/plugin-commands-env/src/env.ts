import { promises as fs } from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import PnpmError from '@pnpm/error'
import cmdShim from '@zkochan/cmd-shim'
import renderHelp from 'render-help'
import { getNodeDir, NvmNodeCommandOptions } from './node'
import resolveNodeVersion from './resolveNodeVersion'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {
    global: Boolean,
  }
}

export const commandNames = ['env']

export function help () {
  return renderHelp({
    description: 'Install and use the specified version of Node.js. The npm CLI bundled with the given Node.js version gets installed as well.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Installs Node.js globally',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('env'),
    usages: [
      'pnpm env use --global <version>',
      'pnpm env use --global 16',
      'pnpm env use --global lts',
      'pnpm env use --global argon',
      'pnpm env use --global latest',
      'pnpm env use --global rc/16',
    ],
  })
}

export async function handler (opts: NvmNodeCommandOptions, params: string[]) {
  if (params.length === 0) {
    throw new PnpmError('ENV_NO_SUBCOMMAND', 'Please specify the subcommand')
  }
  switch (params[0]) {
  case 'use': {
    if (!opts.global) {
      throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
    }
    const { version: nodeVersion, releaseDir } = await resolveNodeVersion(params[1])
    if (!nodeVersion) {
      throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${params[1]}`)
    }
    const nodeDir = await getNodeDir({
      ...opts,
      useNodeVersion: nodeVersion,
      releaseDir,
    })
    const src = path.join(nodeDir, process.platform === 'win32' ? 'node.exe' : 'bin/node')
    const dest = path.join(opts.bin, process.platform === 'win32' ? 'node.exe' : 'node')
    try {
      await fs.unlink(dest)
    } catch (err) {}
    await fs.symlink(src, dest, 'file')
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
    } catch (err: any) { // eslint-disable-line
      // ignore
    }
    return `Node.js ${nodeVersion} is activated
  ${dest} -> ${src}`
  }
  default: {
    throw new PnpmError('ENV_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
