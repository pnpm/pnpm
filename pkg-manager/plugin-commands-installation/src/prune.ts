import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS, OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import * as install from './install'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([
    'dev',
    'optional',
    'production',
    'ignore-scripts',
  ], allTypes)
}

export const commandNames = ['prune']

export function help (): string {
  return renderHelp({
    description: 'Removes extraneous packages',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Remove the packages specified in `devDependencies`',
            name: '--prod',
          },
          {
            description: 'Remove the packages specified in `optionalDependencies`',
            name: '--no-optional',
          },
          OPTIONS.ignoreScripts,
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('prune'),
    usages: ['pnpm prune [--prod]'],
  })
}

export async function handler (
  opts: install.InstallCommandOptions
): Promise<void> {
  return install.handler({
    ...opts,
    modulesCacheMaxAge: 0,
    pruneDirectDependencies: true,
    pruneStore: true,
  })
}
