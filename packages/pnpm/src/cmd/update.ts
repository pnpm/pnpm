import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import { PnpmOptions } from '../types'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from './help'
import { handler as installCmd } from './install'

export function types () {
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
    'latest',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
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
    'use-running-store-server',
  ], allTypes)
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
  opts: PnpmOptions,
) {
  return installCmd(input, { ...opts, update: true, allowNew: false })
}
