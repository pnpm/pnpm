import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { oneLine, stripIndent } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import { handler as list } from './list'

export function types () {
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
    'recursive',
  ], allTypes)
}

export const commandNames = ['why']

export function help () {
  return renderHelp({
    description: stripIndent`
      Shows the packages that depend on <pkg>
      For example: pnpm why babel-* eslint-*`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`Perform command on every package in subdirectories
              or on every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
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
            description: 'Display only the dependency tree for packages in \`dependencies\`',
            name: '--prod, --production',
          },
          {
            description: 'Display only the dependency tree for packages in \`devDependencies\`',
            name: '--dev',
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

export const handler = list
