import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import renderHelp from 'render-help'
import { configSet } from './configSet'
import { ConfigCommandOptions } from './ConfigCommandOptions'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {
    global: Boolean,
  }
}

export const commandNames = ['config', 'c']

export function help () {
  return renderHelp({
    description: 'Manage the pnpm configuration files.',
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
      'pnpm config set <key> <value>',
    ],
  })
}

export async function handler (opts: ConfigCommandOptions, params: string[]) {
  if (params.length === 0) {
    throw new PnpmError('ENV_NO_SUBCOMMAND', 'Please specify the subcommand', {
      hint: help(),
    })
  }
  switch (params[0]) {
  case 'set': {
    return configSet(opts, params[1], params[2])
  }
  default: {
    throw new PnpmError('ENV_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
