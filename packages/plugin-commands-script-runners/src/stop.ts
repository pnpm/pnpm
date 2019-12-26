import { docsUrl } from '@pnpm/cli-utils'
import renderHelp = require('render-help')
import {
  handler as run,
  IF_PRESENT_OPTION,
  IF_PRESENT_OPTION_HELP,
  RunOpts,
} from './run'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return IF_PRESENT_OPTION
}

export const commandNames = ['stop']

export function help () {
  return renderHelp({
    description: `Runs a package's "stop" script, if one was provided.`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          IF_PRESENT_OPTION_HELP,
        ],
      },
    ],
    url: docsUrl('stop'),
    usages: ['pnpm stop [-- <args>...]'],
  })
}

export async function handler (
  args: string[],
  opts: RunOpts,
) {
  return run(['stop', ...args], opts)
}
