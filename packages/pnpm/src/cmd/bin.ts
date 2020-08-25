import { types as allTypes } from '@pnpm/config'
import R = require('ramda')
import renderHelp = require('render-help')

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
    'global',
  ], allTypes)
}

export const commandNames = ['bin']

export function help () {
  return renderHelp({
    description: 'Print the directory where pnpm will install executables.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print the global executables directory',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    usages: ['pnpm bin [-g]'],
  })
}

export async function handler (
  opts: {
    bin: string
  }
) {
  return opts.bin
}
