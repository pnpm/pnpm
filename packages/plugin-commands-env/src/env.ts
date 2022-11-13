import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import renderHelp from 'render-help'
import { envRemove } from './envRemove'
import { envUse } from './envUse'
import { NvmNodeCommandOptions } from './node'
import { envList } from './envList'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {
    global: Boolean,
    remote: Boolean,
  }
}

export const commandNames = ['env']

export function help () {
  return renderHelp({
    description: 'Manage Node.js versions.',
    descriptionLists: [
      {
        title: 'Commands',
        list: [
          {
            description: 'Installs the specified version of Node.js. The npm CLI bundled with the given Node.js version gets installed as well.',
            name: 'use',
          },
          {
            description: 'Removes the specified version of Node.js.',
            name: 'remove',
            shortAlias: 'rm',
          },
          {
            description: 'List Node.js versions available locally or remotely',
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
          {
            description: 'List the remote versions of Node.js',
            name: '--remote',
          },
        ],
      },
    ],
    url: docsUrl('env'),
    usages: [
      'pnpm env [command] [options] <version>',
      'pnpm env use --global 16',
      'pnpm env use --global lts',
      'pnpm env use --global argon',
      'pnpm env use --global latest',
      'pnpm env use --global rc/16',
      'pnpm env remove --global 16',
      'pnpm env remove --global lts',
      'pnpm env remove --global argon',
      'pnpm env remove --global latest',
      'pnpm env remove --global rc/16',
      'pnpm env list',
      'pnpm env list --remote',
      'pnpm env list --remote 16',
      'pnpm env list --remote lts',
      'pnpm env list --remote argon',
      'pnpm env list --remote latest',
      'pnpm env list --remote rc/16',
    ],
  })
}

export async function handler (opts: NvmNodeCommandOptions, params: string[]) {
  if (params.length === 0) {
    throw new PnpmError('ENV_NO_SUBCOMMAND', 'Please specify the subcommand')
  }
  switch (params[0]) {
  case 'use': {
    return envUse(opts, params.slice(1))
  }
  case 'remove':
  case 'rm':
  case 'uninstall':
  case 'un': {
    return envRemove(opts, params.slice(1))
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
