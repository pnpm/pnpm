import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import PnpmError from '@pnpm/error'
import cmdShim from '@zkochan/cmd-shim'
import renderHelp from 'render-help'
import { getNodeDir, NvmNodeCommandOptions } from './node'

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
    description: 'Install and use the specified version of Node.js',
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
    const nodeDir = await getNodeDir({
      ...opts,
      useNodeVersion: params[1],
    })
    const src = path.join(nodeDir, process.platform === 'win32' ? 'node.exe' : 'node')
    const dest = path.join(opts.bin, 'node')
    await cmdShim(src, dest)
    return `Node.js ${params[1]} is activated
  ${dest} -> ${src}`
  }
  default: {
    throw new PnpmError('ENV_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
