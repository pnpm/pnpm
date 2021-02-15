import path from 'path'
import { types as allTypes } from '@pnpm/config'
import * as R from 'ramda'
import renderHelp from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
    'global',
  ], allTypes)
}

export const commandNames = ['root']

export function help () {
  return renderHelp({
    description: 'Print the effective `node_modules` directory.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print the global `node_modules` directory',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    usages: ['pnpm root [-g]'],
  })
}

export async function handler (
  opts: {
    dir: string
  }
) {
  return `${path.join(opts.dir, 'node_modules')}\n`
}
