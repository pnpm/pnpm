import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import renderHelp from 'render-help'
import { configGet } from './configGet'
import { configSet } from './configSet'
import { configList } from './configList'
import { type ConfigCommandOptions } from './ConfigCommandOptions'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {
    global: Boolean,
    location: ['global', 'project'],
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
          {
            description: 'When set to "project", the .npmrc file at the nearest package.json will be used',
            name: '--location <project|global>',
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
  if (opts.location) {
    opts.global = opts.location === 'global'
  } else if (opts.cliOptions['global'] == null) {
    opts.global = true
  }
  switch (params[0]) {
  case 'set': {
    let [key, value] = params.slice(1)
    if (value == null) {
      const parts = key.split('=')
      key = parts.shift()!
      value = parts.join('=')
    }
    return configSet(opts, key, value ?? '')
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
