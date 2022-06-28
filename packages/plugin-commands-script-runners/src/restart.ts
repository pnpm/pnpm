import { types as allTypes } from '@pnpm/config'
import pick from 'ramda/src/pick.js'
import renderHelp from 'render-help'
import {
  handler as run,
  IF_PRESENT_OPTION,
  IF_PRESENT_OPTION_HELP,
  RunOpts,
} from './run'

export function rcOptionsTypes () {
  return {
    ...pick([
      'npm-path',
    ], allTypes),
  }
}

export function cliOptionsTypes () {
  return IF_PRESENT_OPTION
}

export const commandNames = ['restart']

export function help () {
  return renderHelp({
    description: 'Restarts a package. Runs a package\'s "stop", "restart", and "start" scripts, and associated pre- and post- scripts.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          IF_PRESENT_OPTION_HELP,
        ],
      },
    ],
    usages: ['pnpm restart [-- <args>...]'],
  })
}

export async function handler (
  opts: RunOpts,
  params: string[]
) {
  await run(opts, ['stop', ...params])
  await run(opts, ['restart', ...params])
  await run(opts, ['start', ...params])
}
