import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import renderHelp from 'render-help'
import { envList } from './envList.js'
import { envUse } from './envUse.js'
import { type NvmNodeCommandOptions } from './node.js'

export const skipPackageManagerCheck = true

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    global: Boolean,
    remote: Boolean,
  }
}

export const commandNames = ['env']

export function help (): string {
  return renderHelp({
    description: 'Manage Node.js versions.',
    descriptionLists: [
      {
        title: 'Commands',
        list: [
          {
            description: 'Installs the specified version of Node.js. The npm CLI bundled with the given Node.js version gets installed as well. This sets this version of Node.js as the current version.',
            name: 'use',
          },
          {
            description: 'List remote Node.js versions available to install.',
            name: 'list',
            shortAlias: 'ls',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'Manages Node.js versions globally',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('env'),
    usages: [
      'pnpm env use --global 18',
      'pnpm env use --global lts',
      'pnpm env use --global argon',
      'pnpm env use --global latest',
      'pnpm env use --global rc/18',
      'pnpm env list',
      'pnpm env list 18',
      'pnpm env list lts',
      'pnpm env list argon',
      'pnpm env list latest',
      'pnpm env list rc/18',
    ],
  })
}

export async function handler (opts: NvmNodeCommandOptions, params: string[]): Promise<string | { exitCode: number } | void> {
  if (params.length === 0) {
    throw new PnpmError('ENV_NO_SUBCOMMAND', 'Please specify the subcommand', {
      hint: help(),
    })
  }
  if (opts.global && !opts.bin) {
    throw new PnpmError('CANNOT_MANAGE_NODE', 'Unable to manage Node.js because pnpm was not installed using the standalone installation script', {
      hint: 'If you want to manage Node.js with pnpm, you need to remove any Node.js that was installed by other tools, then install pnpm using one of the standalone scripts that are provided on the installation page: https://pnpm.io/installation',
    })
  }
  switch (params[0]) {
  case 'use': {
    await envUse(opts, params.slice(1))
    return
  }
  case 'list':
  case 'ls': {
    return envList(opts, params.slice(1))
  }
  default: {
    throw new PnpmError('ENV_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
