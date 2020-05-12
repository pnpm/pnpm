import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import {
  handler as run,
  IF_PRESENT_OPTION,
  IF_PRESENT_OPTION_HELP,
  RunOpts,
} from './run'

export function rcOptionsTypes () {
  return {
    ...R.pick([
      'npm-path',
    ], allTypes),
  }
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
  opts: RunOpts,
  params: string[]
) {
  return run(opts, ['start', ...params])
}
