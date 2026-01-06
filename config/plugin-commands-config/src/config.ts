import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import renderHelp from 'render-help'
import { configGet } from './configGet.js'
import { configSet } from './configSet.js'
import { configList } from './configList.js'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    global: Boolean,
    location: ['global', 'project'],
    json: Boolean,
  }
}

export const commandNames = ['config', 'c']

export function help (): string {
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
            description: 'Print the config value for the provided key or property path',
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
            description: 'When set to "project", the pnpm-workspace.yaml file will be used if it exists. If only .npmrc exists, it will be used. If neither exists, a pnpm-workspace.yaml file will be created.',
            name: '--location <project|global>',
          },
          {
            description: 'Show all types of values in JSON format (not just objects and arrays)',
            name: '--json',
          },
        ],
      },
    ],
    url: docsUrl('config'),
    usages: [
      'pnpm config set <key> <value>',
      'pnpm config get <key>',
      'pnpm config get --json <key>',
      'pnpm config delete <key>',
      'pnpm config list',
    ],
  })
}

export type ConfigHandlerResult = string | undefined | { output: string, exitCode: number }

export async function handler (opts: ConfigCommandOptions, params: string[]): Promise<ConfigHandlerResult> {
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
  case 'set':
  case 'delete': {
    if (!params[1]) {
      throw new PnpmError('CONFIG_NO_PARAMS', `\`pnpm config ${params[0]}\` requires the config key`)
    }
    if (params[0] === 'set') {
      let [key, value] = params.slice(1)
      if (value == null) {
        const parts = key.split('=')
        key = parts.shift()!
        value = parts.join('=')
      }
      return configSet(opts, key, value ?? '') as Promise<undefined>
    } else {
      return configSet(opts, params[1], null) as Promise<undefined>
    }
  }
  case 'get': {
    if (params[1]) {
      return configGet(opts, params[1])
    } else {
      return configList(opts)
    }
  }
  case 'list': {
    return configList(opts)
  }
  default: {
    throw new PnpmError('CONFIG_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
