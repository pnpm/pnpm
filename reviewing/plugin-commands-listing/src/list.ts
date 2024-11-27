import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { list, listForPackages } from '@pnpm/list'
import { type IncludedDependencies } from '@pnpm/types'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { listRecursive } from './recursive'

export const EXCLUDE_PEERS_HELP = {
  description: 'Exclude peer dependencies',
  name: '--exclude-peers',
}

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
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
  ], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  'exclude-peers': Boolean,
  'only-projects': Boolean,
  recursive: Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['list', 'ls']

export function help (): string {
  return renderHelp({
    aliases: ['list', 'ls', 'la', 'll'],
    description: 'When run as ll or la, it shows extended information by default. \
All dependencies are printed by default. Search by patterns is supported. \
For example: pnpm ls babel-* eslint-*',
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
            description: 'Max display depth of the dependency tree',
            name: '--depth <number>',
          },
          {
            description: 'Display only direct dependencies',
            name: '--depth 0',
          },
          {
            description: 'Display only projects. Useful in a monorepo. `pnpm ls -r --depth -1` lists all projects in a monorepo',
            name: '--depth -1',
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
            description: 'Display only dependencies that are also projects within the workspace',
            name: '--only-projects',
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
    url: docsUrl('list'),
    usages: [
      'pnpm ls [<pkg> ...]',
    ],
  })
}

export type ListCommandOptions = Pick<Config,
| 'allProjects'
| 'dev'
| 'dir'
| 'optional'
| 'production'
| 'selectedProjectsGraph'
| 'modulesDir'
| 'virtualStoreDirMaxLength'
> & Partial<Pick<Config, 'cliOptions'>> & {
  alwaysPrintRootPackage?: boolean
  depth?: number
  excludePeers?: boolean
  json?: boolean
  lockfileDir?: string
  long?: boolean
  parseable?: boolean
  onlyProjects?: boolean
  recursive?: boolean
}

export async function handler (
  opts: ListCommandOptions,
  params: string[]
): Promise<string> {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const depth = opts.cliOptions?.['depth'] ?? 0
  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    return listRecursive(pkgs, params, { ...opts, depth, include })
  }
  return render([opts.dir], params, {
    ...opts,
    depth,
    include,
    lockfileDir: opts.lockfileDir ?? opts.dir,
  })
}

export async function render (
  prefixes: string[],
  params: string[],
  opts: {
    alwaysPrintRootPackage?: boolean
    depth?: number
    excludePeers?: boolean
    include: IncludedDependencies
    lockfileDir: string
    long?: boolean
    json?: boolean
    onlyProjects?: boolean
    parseable?: boolean
    modulesDir?: string
    virtualStoreDirMaxLength: number
  }
): Promise<string> {
  const listOpts = {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth ?? 0,
    excludePeerDependencies: opts.excludePeers,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    long: opts.long,
    onlyProjects: opts.onlyProjects,
    reportAs: (opts.parseable ? 'parseable' : (opts.json ? 'json' : 'tree')) as ('parseable' | 'json' | 'tree'),
    showExtraneous: false,
    modulesDir: opts.modulesDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
  }
  return (params.length > 0)
    ? listForPackages(params, prefixes, listOpts)
    : list(prefixes, listOpts)
}
