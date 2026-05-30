import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { list, listForPackages } from '@pnpm/deps.inspection.list'
import { PnpmError } from '@pnpm/error'
import { findGlobalInstallDirs, listGlobalPackages } from '@pnpm/global.commands'
import type { Finder, IncludedDependencies } from '@pnpm/types'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { BASE_RC_OPTION_KEYS, computeInclude, determineReportAs, resolveFinders, SHARED_CLI_HELP_OPTIONS } from './common.js'
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

export const recursiveByDefault = true

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
| 'dev'
| 'dir'
| 'optional'
| 'production'
| 'modulesDir'
| 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'allProjects'
| 'finders'
| 'selectedProjectsGraph'
> & Partial<Pick<ConfigContext, 'cliOptions'>> & {
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
  const include = computeInclude(opts)
  const depth = opts.cliOptions?.['depth'] ?? 0
  if (opts.global && opts.globalPkgDir) {
    if (depth > 0) {
      const allInstallDirs = findGlobalInstallDirs(opts.globalPkgDir, [])
      if (allInstallDirs.length === 1) {
        // Single global install: delegate with params unchanged so
        // listForPackages can search across the whole tree (including
        // transitive deps), matching regular `pnpm ls` semantics.
        return render([allInstallDirs[0]], params, {
          ...opts,
          depth,
          include,
          lockfileDir: allInstallDirs[0],
          checkWantedLockfileOnly: opts.lockfileOnly,
          onlyProjects: opts.cliOptions?.['only-projects'] ?? opts.onlyProjects,
        })
      }
      // Multiple installs — try to narrow to a single one via params,
      // matching against top-level aliases of each install group.
      const matchingInstallDirs = findGlobalInstallDirs(opts.globalPkgDir, params)
      if (matchingInstallDirs.length > 1 || (matchingInstallDirs.length === 0 && allInstallDirs.length > 0)) {
        throw new PnpmError('GLOBAL_LS_DEPTH_NOT_SUPPORTED',
          'Cannot list a merged dependency tree across multiple global packages. ' +
          'Each global package is installed in an isolated directory with its own lockfile, ' +
          'so transitive dependencies cannot be coherently merged. ' +
          'Filter to a single global package by its top-level name, or omit --depth.')
      }
      if (matchingInstallDirs.length === 1) {
        // Drop params: they served their purpose of narrowing to a single
        // install group. Passing them through to `render` would activate
        // search semantics, which prune the matched package's children.
        return render([matchingInstallDirs[0]], [], {
          ...opts,
          depth,
          include,
          lockfileDir: matchingInstallDirs[0],
          checkWantedLockfileOnly: opts.lockfileOnly,
          onlyProjects: opts.cliOptions?.['only-projects'] ?? opts.onlyProjects,
        })
      }
    }
    return listGlobalPackages(opts.globalPkgDir, params, {
      long: opts.long,
      reportAs: determineReportAs(opts),
    })
  }
  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    return listRecursive(pkgs, params, { ...opts, depth, include, checkWantedLockfileOnly: opts.lockfileOnly, onlyProjects: opts.cliOptions?.['only-projects'] ?? opts.onlyProjects })
  }
  return render([opts.dir], params, {
    ...opts,
    depth,
    include,
    lockfileDir: opts.lockfileDir ?? opts.dir,
    checkWantedLockfileOnly: opts.lockfileOnly,
    onlyProjects: opts.cliOptions?.['only-projects'] ?? opts.onlyProjects,
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
