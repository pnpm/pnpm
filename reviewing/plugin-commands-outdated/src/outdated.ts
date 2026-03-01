import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifestOnly,
  TABLE_OPTIONS,
} from '@pnpm/cli-utils'
import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { type CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { scanGlobalPackages } from '@pnpm/global.packages'
import {
  outdatedDepsOfProjects,
  type OutdatedPackage,
} from '@pnpm/outdated'
import semverDiff from '@pnpm/semver-diff'
import { type DependenciesField, type PackageManifest, type ProjectManifest, type ProjectRootDir } from '@pnpm/types'
import { table } from '@zkochan/table'
import chalk from 'chalk'
import { pick, sortWith } from 'ramda'
import renderHelp from 'render-help'
import { stripVTControlCharacters as stripAnsi } from 'util'
import {
  DEFAULT_COMPARATORS,
  NAME_COMPARATOR,
  type OutdatedWithVersionDiff,
} from './utils.js'
import { outdatedRecursive } from './recursive.js'

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

export function help (): string {
  return renderHelp({
    description: `Check for outdated packages. The check can be limited to a subset of the installed packages by providing arguments (patterns are supported).

Examples:
pnpm outdated
pnpm outdated --long
pnpm outdated gulp-* @babel/core`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
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
            description: 'Specify the sorting method. Currently only `name` is supported.',
            name: '--sort-by',
          },
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
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
  long?: boolean
  recursive?: boolean
  format?: 'table' | 'list' | 'json'
  sortBy?: 'name'
} & Pick<Config,
| 'allProjects'
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
| 'rawConfig'
| 'registries'
| 'selectedProjectsGraph'
| 'strictSsl'
| 'tag'
| 'userAgent'
| 'updateConfig'
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
  let packages: Array<{ rootDir: ProjectRootDir, manifest: ProjectManifest }>
  if (opts.global && opts.globalPkgDir) {
    const globalPackages = scanGlobalPackages(opts.globalPkgDir)
    packages = await Promise.all(
      globalPackages.map(async (pkg) => ({
        rootDir: pkg.installDir as ProjectRootDir,
        manifest: await readProjectManifestOnly(pkg.installDir, opts),
      }))
    )
  } else {
    const manifest = await readProjectManifestOnly(opts.dir, opts)
    packages = [
      {
        rootDir: opts.dir as ProjectRootDir,
        manifest,
      },
    ]
  }
  const outdatedPerProject = await outdatedDepsOfProjects(packages, params, {
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
  const outdatedPackages = outdatedPerProject.flat()

  let output!: string
  switch (opts.format ?? 'table') {
  case 'table': {
    output = renderOutdatedTable(outdatedPackages, opts)
    break
  }
  case 'list': {
    output = renderOutdatedList(outdatedPackages, opts)
    break
  }
  case 'json': {
    output = renderOutdatedJSON(outdatedPackages, opts)
    break
  }
  default: {
    throw new PnpmError('BAD_OUTDATED_FORMAT', `Unsupported format: ${opts.format?.toString() ?? 'undefined'}`)
  }
  }
  return {
    output,
    exitCode: outdatedPackages.length === 0 ? 0 : 1,
  }
}

function renderOutdatedTable (outdatedPackages: readonly OutdatedPackage[], opts: { long?: boolean, sortBy?: 'name' }): string {
  if (outdatedPackages.length === 0) return ''
  const columnNames = [
    'Package',
    'Current',
    'Latest',
  ]

  const columnFns = [
    renderPackageName,
    renderCurrent,
    renderLatest,
  ]

  if (opts.long) {
    columnNames.push('Details')
    columnFns.push(renderDetails)
  }

  // Avoid the overhead of allocating a new array caused by calling `array.map()`
  for (let i = 0; i < columnNames.length; i++)
    columnNames[i] = chalk.blueBright(columnNames[i])

  const data = [
    columnNames,
    ...sortOutdatedPackages(outdatedPackages, { sortBy: opts.sortBy })
      .map((outdatedPkg) => columnFns.map((fn) => fn(outdatedPkg))),
  ]
  const tableOptions = {
    ...TABLE_OPTIONS,
  }
  if (opts.long) {
    const detailsColumnMaxWidth = outdatedPackages.filter(pkg => pkg.latestManifest && !pkg.latestManifest.deprecated).reduce((maxWidth, pkg) => {
      const cellWidth = pkg.latestManifest?.homepage?.length ?? 0
      return Math.max(maxWidth, cellWidth)
    }, 40)
    tableOptions.columns = {
      // Detail column:
      3: {
        width: detailsColumnMaxWidth,
        wrapWord: true,
      },
    }
  }

  return table(data, tableOptions)
}

function renderOutdatedList (outdatedPackages: readonly OutdatedPackage[], opts: { long?: boolean, sortBy?: 'name' }): string {
  if (outdatedPackages.length === 0) return ''
  return sortOutdatedPackages(outdatedPackages, { sortBy: opts.sortBy })
    .map((outdatedPkg) => {
      let info = `${chalk.bold(renderPackageName(outdatedPkg))}
${renderCurrent(outdatedPkg)} ${chalk.grey('=>')} ${renderLatest(outdatedPkg)}`

      if (opts.long) {
        const details = renderDetails(outdatedPkg)

        if (details) {
          info += `\n${details}`
        }
      }

      return info
    })
    .join('\n\n') + '\n'
}

export interface OutdatedPackageJSONOutput {
  current?: string
  latest?: string
  wanted: string
  isDeprecated: boolean
  dependencyType: DependenciesField
  latestManifest?: PackageManifest
}

function renderOutdatedJSON (outdatedPackages: readonly OutdatedPackage[], opts: { long?: boolean, sortBy?: 'name' }): string {
  const outdatedPackagesJSON: Record<string, OutdatedPackageJSONOutput> = sortOutdatedPackages(outdatedPackages, { sortBy: opts.sortBy })
    .reduce((acc, outdatedPkg) => {
      acc[outdatedPkg.packageName] = {
        current: outdatedPkg.current,
        latest: outdatedPkg.latestManifest?.version,
        wanted: outdatedPkg.wanted,
        isDeprecated: Boolean(outdatedPkg.latestManifest?.deprecated),
        dependencyType: outdatedPkg.belongsTo,
      }
      if (opts.long) {
        acc[outdatedPkg.packageName].latestManifest = outdatedPkg.latestManifest
      }
      return acc
    }, {} as Record<string, OutdatedPackageJSONOutput>)
  return JSON.stringify(outdatedPackagesJSON, null, 2)
}

function sortOutdatedPackages (outdatedPackages: readonly OutdatedPackage[], opts?: { sortBy?: 'name' }) {
  const sortBy = opts?.sortBy
  const comparators = (sortBy === 'name') ? [NAME_COMPARATOR] : DEFAULT_COMPARATORS
  return sortWith(
    comparators,
    outdatedPackages.map(toOutdatedWithVersionDiff)
  )
}

export function getCellWidth (data: string[][], columnNumber: number, maxWidth: number): number {
  const maxCellWidth = data.reduce((cellWidth, row) => {
    const cellLines = stripAnsi(row[columnNumber]).split('\n')
    const currentCellWidth = cellLines.reduce((lineWidth, line) => {
      return Math.max(lineWidth, line.length)
    }, 0)
    return Math.max(cellWidth, currentCellWidth)
  }, 0)
  return Math.min(maxWidth, maxCellWidth)
}

export function toOutdatedWithVersionDiff<Pkg extends OutdatedPackage> (outdated: Pkg): Pkg & OutdatedWithVersionDiff {
  if (outdated.latestManifest != null) {
    return {
      ...outdated,
      ...semverDiff.default(outdated.wanted, outdated.latestManifest.version),
    }
  }
  return {
    ...outdated,
    change: 'unknown',
  }
}

export function renderPackageName ({ belongsTo, packageName }: OutdatedPackage): string {
  switch (belongsTo) {
  case 'devDependencies': return `${packageName} ${chalk.dim('(dev)')}`
  case 'optionalDependencies': return `${packageName} ${chalk.dim('(optional)')}`
  default: return packageName
  }
}

export function renderCurrent ({ current, wanted }: OutdatedPackage): string {
  const output = current ?? 'missing'
  if (current === wanted) return output
  return `${output} (wanted ${wanted})`
}

export function renderLatest (outdatedPkg: OutdatedWithVersionDiff): string {
  const { latestManifest, change, diff } = outdatedPkg
  if (latestManifest == null) return ''
  if (change === null || (diff == null)) {
    return latestManifest.deprecated
      ? chalk.redBright.bold('Deprecated')
      : latestManifest.version
  }

  const versionText = colorizeSemverDiff.default({ change, diff })
  if (latestManifest.deprecated) {
    return `${versionText} ${chalk.redBright('(deprecated)')}`
  }

  return versionText
}

export function renderDetails ({ latestManifest }: OutdatedPackage): string {
  if (latestManifest == null) return ''
  const outputs = []
  if (latestManifest.deprecated) {
    outputs.push(chalk.redBright(latestManifest.deprecated))
  }
  if (latestManifest.homepage) {
    outputs.push(chalk.underline(latestManifest.homepage))
  }
  return outputs.join('\n')
}
