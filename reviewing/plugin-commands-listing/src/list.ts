import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { listGlobalPackages } from '@pnpm/global.commands'
import { list, listForPackages } from '@pnpm/list'
import { type Finder, type IncludedDependencies } from '@pnpm/types'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import { computeInclude, resolveFinders, determineReportAs, SHARED_CLI_HELP_OPTIONS, BASE_RC_OPTION_KEYS } from './common.js'
import { listRecursive } from './recursive.js'

export const EXCLUDE_PEERS_HELP = {
  description: 'Exclude peer dependencies',
  name: '--exclude-peers',
}

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    ...BASE_RC_OPTION_KEYS,
    'depth',
    'lockfile-only',
  ], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  'exclude-peers': Boolean,
  'only-projects': Boolean,
  recursive: Boolean,
  'find-by': [String, Array],
})

export { shorthands } from './common.js'

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
          ...SHARED_CLI_HELP_OPTIONS,
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
            description: 'Display only dependencies that are also projects within the workspace',
            name: '--only-projects',
          },
          {
            description: 'List packages from the lockfile only, without checking node_modules.',
            name: '--lockfile-only',
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
| 'finders'
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
  lockfileOnly?: boolean
  long?: boolean
  parseable?: boolean
  onlyProjects?: boolean
  recursive?: boolean
  findBy?: string[]
} & Partial<Pick<Config, 'global' | 'globalPkgDir'>>

export async function handler (
  opts: ListCommandOptions,
  params: string[]
): Promise<string> {
  if (opts.global && opts.globalPkgDir) {
    return listGlobalPackages(opts.globalPkgDir, params)
  }
  const include = computeInclude(opts)
  const depth = opts.cliOptions?.['depth'] ?? 0
  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    return listRecursive(pkgs, params, { ...opts, depth, include, checkWantedLockfileOnly: opts.lockfileOnly })
  }
  return render([opts.dir], params, {
    ...opts,
    depth,
    include,
    lockfileDir: opts.lockfileDir ?? opts.dir,
    checkWantedLockfileOnly: opts.lockfileOnly,
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
    checkWantedLockfileOnly?: boolean
    long?: boolean
    json?: boolean
    onlyProjects?: boolean
    parseable?: boolean
    modulesDir?: string
    virtualStoreDirMaxLength: number
    finders?: Record<string, Finder>
    findBy?: string[]
  }
): Promise<string> {
  const finders = resolveFinders(opts)
  const listOpts = {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth ?? 0,
    excludePeerDependencies: opts.excludePeers,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    checkWantedLockfileOnly: opts.checkWantedLockfileOnly,
    long: opts.long,
    onlyProjects: opts.onlyProjects,
    reportAs: determineReportAs(opts),
    showExtraneous: false,
    showSummary: true,
    modulesDir: opts.modulesDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    finders,
  }
  return (params.length > 0) || listOpts.finders.length > 0
    ? listForPackages(params, prefixes, listOpts)
    : list(prefixes, listOpts)
}
