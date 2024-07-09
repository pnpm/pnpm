import {
  docsUrl,
  readDepNameCompletions,
} from '@pnpm/cli-utils'
import { type CompletionFunc } from '@pnpm/command'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { type LicensesCommandResult } from './LicensesCommandResult'
import { licensesList, type LicensesCommandOptions } from './licensesList'

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick(
      ['dev', 'global-dir', 'global', 'json', 'long', 'optional', 'production'],
      allTypes
    ),
    compatible: Boolean,
    table: Boolean,
  }
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['licenses']

export function help (): string {
  return renderHelp({
    description: 'Check the licenses of the installed packages.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description:
              'Show more details (such as a link to the repo) are not displayed. \
To display the details, pass this option.',
            name: '--long',
          },
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
          {
            description: 'Check only "dependencies" and "optionalDependencies"',
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Check only "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Don\'t check "optionalDependencies"',
            name: '--no-optional',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('licenses'),
    usages: [
      'pnpm licenses ls',
      'pnpm licenses ls --long',
      'pnpm licenses list',
      'pnpm licenses list --long',
    ],
  })
}

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export async function handler (
  opts: LicensesCommandOptions,
  params: string[] = []
): Promise<LicensesCommandResult> {
  if (params.length === 0) {
    throw new PnpmError('LICENCES_NO_SUBCOMMAND', 'Please specify the subcommand', {
      hint: help(),
    })
  }
  switch (params[0]) {
  case 'list':
  case 'ls':
    return licensesList(opts)
  default: {
    throw new PnpmError('LICENSES_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
