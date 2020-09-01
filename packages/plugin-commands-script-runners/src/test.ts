import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING } from '@pnpm/common-cli-options-help'
import * as run from './run'
import renderHelp = require('render-help')

export const commandNames = ['test', 't', 'tst']

export function help () {
  return renderHelp({
    aliases: ['t', 'tst'],
    description: 'Runs a package\'s "test" script, if one was provided.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: '\
Run the tests in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
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

export function handler (
  opts: run.RunOpts,
  params: string[] = []
) {
  return run.handler(opts, ['test', ...params])
}
