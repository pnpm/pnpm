import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING } from '@pnpm/common-cli-options-help'
import renderHelp from 'render-help'
import * as run from './run'

export const commandNames = ['test', 't', 'tst']

export function help (): string {
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

export async function handler (
  opts: run.RunOpts,
  params: string[] = []
): Promise<string | { exitCode: number } | undefined> {
  return run.handler(opts, ['test', ...params])
}
