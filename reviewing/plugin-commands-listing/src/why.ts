import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { whyForPackages } from '@pnpm/list'
import { type Finder } from '@pnpm/types'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import { type ListCommandOptions, EXCLUDE_PEERS_HELP } from './list.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
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

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  'exclude-peers': Boolean,
  recursive: Boolean,
  'find-by': [String, Array],
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['why']

export function help (): string {
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
            shortAlias: '-P',
          },
          {
            description: 'Display only the dependency graph for packages in `devDependencies`',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: "Don't display packages from `optionalDependencies`",
            name: '--no-optional',
          },
          EXCLUDE_PEERS_HELP,
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

export async function handler (
  opts: ListCommandOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0 && opts.findBy == null) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm why` requires the package name or --find-by=<finder-name>')
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  const finders: Finder[] = []
  if (opts.findBy) {
    for (const finderName of opts.findBy) {
      if (opts.finders?.[finderName] == null) {
        throw new PnpmError('FINDER_NOT_FOUND', `No finder with name ${finderName} is found`)
      }
      finders.push(opts.finders[finderName])
    }
  }

  const lockfileDir = opts.lockfileDir ?? opts.dir

  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    const projectPaths = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package.rootDir)
    return whyForPackages(params, projectPaths, {
      include,
      lockfileDir,
      reportAs: opts.parseable ? 'parseable' : (opts.json ? 'json' : 'tree'),
      modulesDir: opts.modulesDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      checkWantedLockfileOnly: opts.lockfileOnly,
      finders,
    })
  }

  return whyForPackages(params, [opts.dir], {
    include,
    lockfileDir,
    reportAs: opts.parseable ? 'parseable' : (opts.json ? 'json' : 'tree'),
    modulesDir: opts.modulesDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    checkWantedLockfileOnly: opts.lockfileOnly,
    finders,
  })
}
