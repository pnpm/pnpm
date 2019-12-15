import { docsUrl } from '@pnpm/cli-utils'
import { oneLine } from 'common-tags'
import renderHelp = require('render-help')
import { handler as run, IF_PRESENT_OPTION, IF_PRESENT_OPTION_HELP } from './run'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return IF_PRESENT_OPTION
}

export const commandNames = ['start']

export function help () {
  return renderHelp({
    description: oneLine`
      Runs an arbitrary command specified in the package's "start" property of its "scripts" object.
      If no "start" property is specified on the "scripts" object, it will run node server.js.`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          IF_PRESENT_OPTION_HELP,
        ],
      },
    ],
    url: docsUrl('start'),
    usages: ['pnpm start [-- <args>...]'],
  })
}

export async function handler (
  args: string[],
  opts: {
    extraBinPaths: string[],
    dir: string,
    rawConfig: object,
  },
) {
  return run(['start', ...args], opts)
}
