import { docsUrl } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import list, { forPackages as listForPackages } from '@pnpm/list'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from './help'

export function types () {
  return R.pick([
    'depth',
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

export const commandNames = ['list', 'ls', 'la', 'll']

export function help () {
  return renderHelp({
    aliases: ['list', 'ls', 'la', 'll'],
    description: oneLine`When run as ll or la, it shows extended information by default.
      All dependencies are printed by default. Search by patterns is supported.
      For example: pnpm ls babel-* eslint-*`,
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
            description: 'Max display depth of the dependency tree',
            name: '--depth <number>',
          },
          {
            description: 'Display only direct dependencies',
            name: '--depth 0',
          },
          {
            description: 'Display only projects. Useful in a monorepo. \`pnpm ls -r --depth -1\` lists all projects in a monorepo',
            name: '--depth -1',
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
    url: docsUrl('list'),
    usages: [
      'pnpm ls [<pkg> ...]',
    ],
  })
}

export async function handler (
  args: string[],
  opts: Config & {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDir?: string,
    long?: boolean,
    parseable?: boolean,
  },
  command: string,
) {
  const output = await render([opts.dir], args, {
    ...opts,
    lockfileDir: opts.lockfileDir || opts.dir,
  }, command)

  if (output) console.log(output)
}

export async function render (
  prefixes: string[],
  args: string[],
  opts: Config & {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDir: string,
    long?: boolean,
    json?: boolean,
    parseable?: boolean,
  },
  command: string,
) {
  const isWhy = command === 'why'
  if (isWhy && !args.length) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm why` requires the package name')
  }
  opts.long = opts.long || command === 'll' || command === 'la'
  const listOpts = {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: isWhy ? Infinity : opts.depth || 0,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    long: opts.long,
    // tslint:disable-next-line: no-unnecessary-type-assertion
    reportAs: (opts.parseable ? 'parseable' : (opts.json ? 'json' : 'tree')) as ('parseable' | 'json' | 'tree'),
  }
  return isWhy || args.length
    ? listForPackages(args, prefixes, listOpts)
    : list(prefixes, listOpts)
}
