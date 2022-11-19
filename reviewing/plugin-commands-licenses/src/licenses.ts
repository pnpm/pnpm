import {
  docsUrl,
  readDepNameCompletions,
} from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import {
  FILTERING,
  OPTIONS,
  UNIVERSAL_OPTIONS,
} from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { licensesList, LicensesCommandOptions } from './licensesList'

export function rcOptionsTypes () {
  return {
    ...pick(
      ['dev', 'global-dir', 'global', 'json', 'long', 'optional', 'production'],
      allTypes
    ),
    compatible: Boolean,
    table: Boolean,
  }
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
})

export const shorthands = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['licenses']

export function help () {
  return renderHelp({
    description: `Check for licenses packages. The check can be limited to a subset of the installed packages by providing arguments (patterns are supported).

Examples:
pnpm licenses list
pnpm licenses list --long`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description:
              'By default, details about the packages (such as a link to the repo) are not displayed. \
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
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('licenses'),
    usages: ['pnpm licenses [options]'],
  })
}

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export async function handler (
  opts: LicensesCommandOptions,
  params: string[] = []
) {
  if (params.length === 0) {
    throw new PnpmError('LICENCES_NO_SUBCOMMAND', 'Please specify the subcommand')
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
