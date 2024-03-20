import path from 'node:path'

import pick from 'ramda/src/pick'
import renderHelp from 'render-help'

import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes() {
  return pick(['global'], allTypes)
}

export const commandNames = ['root']

export function help(): string {
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
    url: docsUrl('root'),
    usages: ['pnpm root [-g]'],
  })
}

export async function handler(opts: { dir: string }): Promise<string> {
  return `${path.join(opts.dir, 'node_modules')}\n`
}
