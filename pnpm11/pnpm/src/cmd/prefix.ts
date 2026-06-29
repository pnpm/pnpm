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
    globalPrefix?: string
    prefix?: string
  }
): Promise<string> {
  if (opts.global) {
    if (opts.globalPrefix) {
      return opts.globalPrefix
    }
    if (opts.prefix) {
      return opts.prefix
    }
    let prefix = process.env.PREFIX
    if (!prefix) {
      if (process.platform === 'win32') {
        prefix = process.env.APPDATA ? path.join(process.env.APPDATA, 'npm') : path.dirname(process.execPath)
      } else {
        const binDir = path.dirname(process.execPath)
        if (path.basename(binDir) === 'bin') {
          prefix = path.dirname(binDir)
        } else {
          prefix = binDir
        }
      }
    }
    return prefix
  }
  return opts.dir
}
