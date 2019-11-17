import { types as allTypes } from '@pnpm/config'
import path = require('path')
import R = require('ramda')
import renderHelp = require('render-help')

export function types () {
  return R.pick([
    'global',
  ], allTypes)
}

export const commandNames = ['root']

export function help () {
  return renderHelp({
    description: 'Print the effective \`node_modules\` directory.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print the global \`node_modules\` directory',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    usages: ['pnpm root [-g [--independent-leaves]]'],
  })
}

export async function handler (
  args: string[],
  opts: {
    dir: string,
  },
  command: string,
) {
  return `${path.join(opts.dir, 'node_modules')}\n`
}
