import type { CompletionFunc } from '@pnpm/cli.command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli.utils'
import { createMatcher } from '@pnpm/config.matcher'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { findOutdatedGitHubActions, isGitHubActionSelector, normalizeGitHubActionSelector, shouldCheckGitHubActions } from '@pnpm/deps.github-actions'
import { outdatedDepsOfProjects } from '@pnpm/deps.inspection.outdated'
import { PnpmError } from '@pnpm/error'
import { scanGlobalPackages } from '@pnpm/global.packages'
import type { IncludedDependencies, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { outdatedRecursive } from './recursive.js'
import {
  type OutdatedFormat,
  type OutdatedItem,
  renderOutdatedJSON,
  renderOutdatedList,
  renderOutdatedTable,
  selectOutdatedRenderer,
  toOutdatedAction,
} from './render.js'

export {
  createOutdatedJSONKeyGetter,
  getCellWidth,
  type OutdatedItem,
  type OutdatedPackageJSONOutput,
  renderCurrent,
  renderDetails,
  renderLatest,
  renderPackageName,
  toOutdatedAction,
  toOutdatedWithVersionDiff,
} from './render.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'depth',
      'dev',
      'global-dir',
      'global',
      'long',
      'optional',
      'production',
    ], allTypes),
    compatible: Boolean,
    format: ['table', 'list', 'json'],
    'sort-by': 'name',
  }
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  'include-github-actions': Boolean,
  recursive: Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
  table: '--format=table',
  'no-table': '--format=list',
  json: '--format=json',
}

export const commandNames = ['outdated']

const OUTDATED_HELP_OPTIONS = [
  {
    description: 'Print only versions that satisfy specs in package.json',
    name: '--compatible',
  },
  {
    description: 'By default, details about the outdated packages (such as a link to the repo) are not displayed. \
To display the details, pass this option.',
    name: '--long',
  },
  {
    description: 'Check for outdated dependencies in every package found in subdirectories \
or in every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
    name: '--recursive',
    shortAlias: '-r',
  },
  {
    description: 'Prints the outdated packages in a list. Good for small consoles',
    name: '--no-table',
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
  {
    description: 'Prints the outdated dependencies in the given format. Default is "table". Supported options: "table, list, json"',
    name: '--format <format>',
  },
  {
    description: 'Also check GitHub Actions dependencies in workflow and action files',
    name: '--include-github-actions',
  },
  {
    description: 'Specify the sorting method. Currently only `name` is supported.',
    name: '--sort-by',
  },
  OPTIONS.globalDir,
  ...UNIVERSAL_OPTIONS,
]

export function help (): string {
  return renderHelp({
    description: `Check for outdated package dependencies. GitHub Actions dependencies can be included with --include-github-actions. The check can be limited to a subset of dependencies by providing arguments (patterns are supported).

Examples:
pnpm outdated
pnpm outdated --long
pnpm outdated gulp-* @babel/core`,
    descriptionLists: [
      {
        title: 'Options',

        list: OUTDATED_HELP_OPTIONS,
      },
      FILTERING,
    ],
    url: docsUrl('outdated'),
    usages: ['pnpm outdated [<pkg> ...]'],
  })
}

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export type OutdatedCommandOptions = {
  compatible?: boolean
  includeGithubActions?: boolean
  long?: boolean
  recursive?: boolean
  format?: OutdatedFormat
  sortBy?: 'name'
} & Pick<Config,
| 'ca'
| 'cacheDir'
| 'catalogs'
| 'cert'
| 'dev'
| 'dir'
| 'engineStrict'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'global'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'lockfileDir'
| 'minimumReleaseAge'
| 'minimumReleaseAgeExclude'
| 'networkConcurrency'
| 'noProxy'
| 'offline'
| 'optional'
| 'production'
| 'configByUri'
| 'registriesByScope'
| 'strictSsl'
| 'tag'
| 'userAgent'
| 'updateConfig'
| 'workspaceDir'
> & Pick<ConfigContext,
| 'allProjects'
| 'selectedProjectsGraph'
> & Partial<Pick<Config, 'globalPkgDir' | 'userConfig'>>

export async function handler (
  opts: OutdatedCommandOptions,
  params: string[] = []
): Promise<{ output: string, exitCode: number }> {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    return outdatedRecursive(pkgs, params, { ...opts, include })
  }
  const packages = await readProjectsToCheck(opts)
  const packageParams = params.filter((param) => !isGitHubActionSelector(param))
  if (hasUnmatchedPackageParams(packages, packageParams, include)) {
    throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
      'None of the specified packages were found in the dependencies.')
  }
  const outdatedPackages = await findOutdatedItems(packages, { opts, params, packageParams, include })
  const renderOutdated = selectOutdatedRenderer(opts.format, {
    table: renderOutdatedTable,
    list: renderOutdatedList,
    json: renderOutdatedJSON,
  })
  return {
    output: renderOutdated(outdatedPackages, opts),
    exitCode: outdatedPackages.length === 0 ? 0 : 1,
  }
}

type ProjectToCheck = { rootDir: ProjectRootDir, manifest: ProjectManifest }

async function readProjectsToCheck (opts: OutdatedCommandOptions): Promise<ProjectToCheck[]> {
  if (opts.global && opts.globalPkgDir) {
    const globalPackages = scanGlobalPackages(opts.globalPkgDir)
    return Promise.all(
      globalPackages.map(async (pkg) => ({
        rootDir: pkg.installDir as ProjectRootDir,
        manifest: await readProjectManifestOnly(pkg.installDir, opts),
      }))
    )
  }
  const manifest = await readProjectManifestOnly(opts.dir, opts)
  return [
    {
      rootDir: opts.dir as ProjectRootDir,
      manifest,
    },
  ]
}

interface FindOutdatedItemsContext {
  opts: OutdatedCommandOptions
  params: string[]
  packageParams: string[]
  include: IncludedDependencies
}

async function findOutdatedItems (
  packages: ProjectToCheck[],
  { opts, params, packageParams, include }: FindOutdatedItemsContext
): Promise<OutdatedItem[]> {
  const [outdatedPerProject, outdatedActions] = await Promise.all([
    params.length === 0 || packageParams.length > 0
      ? outdatedDepsOfProjects(packages, packageParams, {
        ...opts,
        fullMetadata: opts.long,
        ignoreDependencies: opts.updateConfig?.ignoreDependencies,
        include,
        minimumReleaseAge: opts.minimumReleaseAge,
        minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
        retry: {
          factor: opts.fetchRetryFactor,
          maxTimeout: opts.fetchRetryMaxtimeout,
          minTimeout: opts.fetchRetryMintimeout,
          retries: opts.fetchRetries,
        },
        timeout: opts.fetchTimeout,
      })
      : [],
    opts.global || !include.devDependencies || !shouldCheckGitHubActions(opts)
      ? []
      : findOutdatedGitHubActions({
        compatible: opts.compatible,
        dir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
        match: params.length > 0 ? createMatcher(params.map(normalizeGitHubActionSelector)) : undefined,
        minimumReleaseAge: opts.minimumReleaseAge,
        minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
        serverUrl: opts.updateConfig?.githubActionsServer,
      }),
  ])
  return [
    ...outdatedPerProject.flat(),
    ...outdatedActions.map(toOutdatedAction),
  ]
}

const DEPENDENCY_FIELDS_TO_MATCH = ['dependencies', 'devDependencies', 'optionalDependencies'] as const

export function hasUnmatchedPackageParams (
  pkgs: Array<{ manifest: ProjectManifest }>,
  packageParams: string[],
  include: IncludedDependencies
): boolean {
  const positiveParams = packageParams.filter((param) => !param.startsWith('!'))
  if (positiveParams.length === 0) return false
  const deps = collectIncludedDependencyNames(pkgs, include)
  return positiveParams.some((param) => {
    const matcher = createMatcher([param])
    return !deps.some((dep) => matcher(dep))
  })
}

function collectIncludedDependencyNames (
  pkgs: Array<{ manifest: ProjectManifest }>,
  include: IncludedDependencies
): string[] {
  const availableDeps = new Set<string>()
  for (const { manifest } of pkgs) {
    for (const field of DEPENDENCY_FIELDS_TO_MATCH) {
      if (!include[field] || !manifest[field]) continue
      for (const dep of Object.keys(manifest[field])) availableDeps.add(dep)
    }
  }
  return Array.from(availableDeps)
}
