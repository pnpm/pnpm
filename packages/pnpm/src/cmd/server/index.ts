import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import { PnpmOptions } from '../../types'
import { OPTIONS, UNIVERSAL_OPTIONS } from '../help'
import start from './start'
import status from './status'
import stop from './stop'

export function types () {
  return R.pick([
    'background',
    'ignore-stop-requests',
    'ignore-upload-requests',
    'port',
    'protocol',
    'store',
    'store-dir',
  ], allTypes)
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
            description: oneLine`
              Starts a service that does all interactions with the store.
              Other commands will delegate any store-related tasks to this service`,
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
            description: 'Disallows stopping the server using \`pnpm server stop\`',
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

export async function handler (
  input: string[],
  opts: PnpmOptions & {
    protocol?: 'auto' | 'tcp' | 'ipc',
    port?: number,
    unstoppable?: boolean,
  },
) {
  switch (input[0]) {
    case 'start':
      return start(opts)
    case 'status':
      return status(opts)
    case 'stop':
      return stop(opts)
    default:
      help()
      if (input[0]) {
        throw new PnpmError('INVALID_SERVER_COMMAND', `"server ${input[0]}" is not a pnpm command. See "pnpm help server".`)
      }
  }
}
