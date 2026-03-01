import path from 'path'
import { types as allTypes } from '@pnpm/config'
import { docsUrl } from '@pnpm/cli-utils'
import { pick } from 'ramda'
import renderHelp from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([
    'global',
  ], allTypes)
}

export const commandNames = ['root']

export function help (): string {
  return renderHelp({
    description: 'Print the effective `node_modules` directory.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print the global packages directory',
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

export async function handler (
  opts: {
    dir: string
    global?: boolean
  }
): Promise<string> {
  if (opts.global) {
    return `${opts.dir}\n`
  }
  return `${path.join(opts.dir, 'node_modules')}\n`
}
