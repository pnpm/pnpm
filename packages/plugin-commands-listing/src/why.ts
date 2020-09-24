import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import { handler as list, ListCommandOptions } from './list'
import R = require('ramda')
import renderHelp = require('render-help')

export function rcOptionsTypes () {
  return R.pick([
    'dev',
    'global-dir',
    'global',
    'json',
    'long',
    'only',
    'optional',
    'parseable',
    'production',
  ], allTypes)
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
})

export const commandNames = ['why']

export function help () {
  return renderHelp({
    description: `Shows the packages that depend on <pkg>
For example: pnpm why babel-* eslint-*`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Perform command on every package in subdirectories \
or on every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Show extended information',
            name: '--long',
          },
          {
            description: 'Show parseable output instead of tree view',
            name: '--parseable',
          },
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
          {
            description: 'List packages in the global install prefix instead of in the current project',
            name: '--global',
            shortAlias: '-g',
          },
          {
            description: 'Display only the dependency graph for packages in `dependencies` and `optionalDependencies`',
            name: '--prod',
          },
          {
            description: 'Display only the dependency graph for packages in `devDependencies`',
            name: '--dev',
          },
          {
            description: "Don't display packages from `optionalDependencies`",
            name: '--no-optional',
          },
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('why'),
    usages: [
      'pnpm why <pkg> ...',
    ],
  })
}

export function handler (
  opts: ListCommandOptions,
  params: string[]
) {
  if (!params.length) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm why` requires the package name')
  }
  return list({
    ...opts,
    cliOptions: {
      ...(opts.cliOptions ?? {}),
      depth: Infinity,
    },
  }, params)
}
