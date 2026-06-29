import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([
    'global',
  ], allTypes)
}

export const commandNames = ['prefix']

export function help (): string {
  return renderHelp({
    description: 'Print the current package prefix.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print the global prefix',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('prefix'),
    usages: ['pnpm prefix [-g]'],
  })
}

export async function handler (
  opts: {
    dir: string
    global?: boolean
    globalPkgDir?: string
  }
): Promise<string> {
  if (opts.global) {
    return opts.globalPkgDir ? path.dirname(opts.globalPkgDir) : ''
  }
  return opts.dir
}
