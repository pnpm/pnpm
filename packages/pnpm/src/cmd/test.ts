import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import { handler as run } from './run'

export function types () {
  return R.pick([
    'recursive',
  ], allTypes)
}

export const commandNames = ['test', 't', 'tst']

export function help () {
  return renderHelp({
    aliases: ['t', 'tst'],
    description: `Runs a package's "test" script, if one was provided.`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`
              Run the tests in every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('test'),
    usages: ['pnpm test [-- <args>...]'],
  })
}

export async function handler (
  args: string[],
  opts: {
    extraBinPaths: string[],
    dir: string,
    rawConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['test', ...args], opts)
}
