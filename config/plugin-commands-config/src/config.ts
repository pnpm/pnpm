import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import renderHelp from 'render-help'
import { configGet } from './configGet'
import { configSet } from './configSet'
import { configList } from './configList'
import { ConfigCommandOptions } from './ConfigCommandOptions'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {
    global: Boolean,
    json: Boolean,
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
            description: 'Set the config key to the value provided',
            name: 'set',
          },
          {
            description: 'Print the config value for the provided key',
            name: 'get',
          },
          {
            description: 'Remove the config key from the config file',
            name: 'delete',
          },
          {
            description: 'Show all the config settings',
            name: 'list',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'Sets the configuration in the global config file',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('config'),
    usages: [
      'pnpm config set <key> <value>',
      'pnpm config get <key>',
      'pnpm config delete <key>',
      'pnpm config list',
    ],
  })
}

export async function handler (opts: ConfigCommandOptions, params: string[]) {
  if (params.length === 0) {
    throw new PnpmError('CONFIG_NO_SUBCOMMAND', 'Please specify the subcommand', {
      hint: help(),
    })
  }
  switch (params[0]) {
  case 'set': {
    return configSet(opts, params[1], params[2] ?? '')
  }
  case 'get': {
    return configGet(opts, params[1])
  }
  case 'delete': {
    return configSet(opts, params[1], null)
  }
  case 'list': {
    return configList(opts)
  }
  default: {
    throw new PnpmError('CONFIG_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
