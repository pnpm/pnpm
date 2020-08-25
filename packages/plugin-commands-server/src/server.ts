import { docsUrl } from '@pnpm/cli-utils'
import { OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import { CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import start from './start'
import status from './status'
import stop from './stop'
import R = require('ramda')
import renderHelp = require('render-help')

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {
    ...R.pick([
      'store',
      'store-dir',
    ], allTypes),
    background: Boolean,
    'ignore-stop-requests': Boolean,
    'ignore-upload-requests': Boolean,
    port: Number,
    protocol: ['auto', 'tcp', 'ipc'],
  }
}

export const commandNames = ['server']

export function help () {
  return renderHelp({
    description: 'Manage a store server',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: '\
Starts a service that does all interactions with the store. \
Other commands will delegate any store-related tasks to this service',
            name: 'start',
          },
          {
            description: 'Stops the store server',
            name: 'stop',
          },
          {
            description: 'Prints information about the running server',
            name: 'status',
          },
        ],
      },
      {
        title: 'Start options',

        list: [
          {
            description: 'Runs the server in the background',
            name: '--background',
          },
          {
            description: 'The communication protocol used by the server',
            name: '--protocol <auto|tcp|ipc>',
          },
          {
            description: 'The port number to use, when TCP is used for communication',
            name: '--port <number>',
          },
          OPTIONS.storeDir,
          {
            description: 'Maximum number of concurrent network requests',
            name: '--network-concurrency <number>',
          },
          {
            description: "If false, doesn't check whether packages in the store were mutated",
            name: '--[no-]verify-store-integrity',
          },
          {
            name: '--[no-]lock',
          },
          {
            description: 'Disallows stopping the server using `pnpm server stop`',
            name: '--ignore-stop-requests',
          },
          {
            description: 'Disallows creating new side effect cache during install',
            name: '--ignore-upload-requests',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('server'),
    usages: ['pnpm server <command>'],
  })
}

export function handler (
  opts: CreateStoreControllerOptions & {
    protocol?: 'auto' | 'tcp' | 'ipc'
    port?: number
    unstoppable?: boolean
  },
  params: string[]
) {
  switch (params[0]) {
  case 'start':
    return start(opts)
  case 'status':
    return status(opts)
  case 'stop':
    return stop(opts)
  default:
    help()
    if (params[0]) {
      throw new PnpmError('INVALID_SERVER_COMMAND', `"server ${params[0]}" is not a pnpm command. See "pnpm help server".`)
    }
    return undefined
  }
}
