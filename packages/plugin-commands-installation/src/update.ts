import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import { handler as install, InstallCommandOptions } from './install'

export function rcOptionsTypes () {
  return R.pick([
    'depth',
    'dev',
    'engine-strict',
    'force',
    'global-dir',
    'global-pnpmfile',
    'global',
    'ignore-pnpmfile',
    'ignore-scripts',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'npmPath',
    'offline',
    'only',
    'optional',
    'package-import-method',
    'pnpmfile',
    'prefer-offline',
    'production',
    'recursive',
    'registry',
    'reporter',
    'resolution-strategy',
    'save',
    'save-exact',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store',
    'store-dir',
    'unsafe-perm',
    'use-running-store-server',
  ], allTypes)
}

export function cliOptionsTypes () {
  return {
    ...rcOptionsTypes(),
    latest: Boolean,
    workspace: Boolean,
  }
}

export const commandNames = ['update', 'up', 'upgrade']

export function help () {
  return renderHelp({
    aliases: ['up', 'upgrade'],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`Update in every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Update globally installed packages',
            name: '--global',
            shortAlias: '-g',
          },
          {
            description: 'How deep should levels of dependencies be inspected. 0 is default, which means top-level dependencies',
            name: '--depth <number>',
          },
          {
            description: 'Ignore version ranges in package.json',
            name: '--latest',
            shortAlias: '-L',
          },
          {
            description: 'Update packages only in "dependencies" and "optionalDependencies"',
            name: '--prod',
          },
          {
            description: 'Update packages only in "devDependencies"',
            name: '--dev',
          },
          {
            description: `Don't update packages in "optionalDependencies"`,
            name: '--no-optional',
          },
          {
            description:  oneLine`Tries to link all packages from the workspace.
              Versions are updated to match the versions of packages inside the workspace.
              If specific packages are updated, the command will fail if any of the updated
              dependencies is not found inside the workspace`,
            name: '--workspace',
          },
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('update'),
    usages: ['pnpm update [-g] [<pkg>...]'],
  })
}

export async function handler (
  input: string[],
  opts: InstallCommandOptions,
) {
  return install(input, { ...opts, update: true, allowNew: false })
}
