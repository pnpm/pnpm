import chalk from 'chalk'
import wrapAnsi from 'wrap-ansi'
import pick from 'ramda/src/pick'
import stripAnsi from 'strip-ansi'
import renderHelp from 'render-help'
import { table } from '@zkochan/table'
import sortWith from 'ramda/src/sortWith'

import {
  docsUrl,
  TABLE_OPTIONS,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli-utils'
import {
  OPTIONS,
  FILTERING,
  UNIVERSAL_OPTIONS,
} from '@pnpm/common-cli-options-help'
import { PnpmError } from '@pnpm/error'
import semverDiff from '@pnpm/semver-diff'
import { types as allTypes } from '@pnpm/config'
import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { outdatedDepsOfProjects, type OutdatedPackage } from '@pnpm/outdated'
import type { CompletionFunc, Project, OutdatedCommandOptions, OutdatedPackageJSONOutput } from '@pnpm/types'

import { outdatedRecursive } from './recursive'
import { DEFAULT_COMPARATORS, type OutdatedWithVersionDiff } from './utils'

export function rcOptionsTypes() {
  return {
    ...pick(
      [
        'depth',
        'dev',
        'global-dir',
        'global',
        'long',
        'optional',
        'production',
      ],
      allTypes
    ),
    compatible: Boolean,
    format: ['table', 'list', 'json'],
  }
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
})

export const shorthands = {
  D: '--dev',
  P: '--production',
  table: '--format=table',
  'no-table': '--format=list',
  json: '--format=json',
}

export const commandNames = ['outdated']

export function help(): string {
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
            description:
              'Print only versions that satisfy specs in package.json',
            name: '--compatible',
          },
          {
            description:
              'By default, details about the outdated packages (such as a link to the repo) are not displayed. \
To display the details, pass this option.',
            name: '--long',
          },
          {
            description:
              'Check for outdated dependencies in every package found in subdirectories \
or in every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description:
              'Prints the outdated packages in a list. Good for small consoles',
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

export const completion: CompletionFunc = async (cliOpts): Promise<{
  name: string;
}[]> => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export async function handler(
  opts: OutdatedCommandOptions,
  params: string[] = []
): Promise<{
    output: string;
    exitCode: number;
  }> {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  if (opts.recursive && opts.selectedProjectsGraph != null) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map(
      (wsPkg: {
        dependencies: string[];
        package: Project;
      }): Project => {
        return wsPkg.package;
      }
    )

    // @ts-ignore
    return outdatedRecursive(pkgs, params, { ...opts, include })
  }

  const manifest = await readProjectManifestOnly(opts.dir, opts)

  const packages = [
    {
      dir: opts.dir,
      manifest,
    },
  ]

  const [outdatedPackages] = await outdatedDepsOfProjects(packages, params, {
    ...opts,
    fullMetadata: opts.long,
    ignoreDependencies: manifest?.pnpm?.updateConfig?.ignoreDependencies,
    include,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })

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
      throw new PnpmError(
        'BAD_OUTDATED_FORMAT',
        `Unsupported format: ${opts.format?.toString() ?? 'undefined'}`
      )
    }
  }

  return {
    output,
    exitCode: outdatedPackages.length === 0 ? 0 : 1,
  }
}

function renderOutdatedTable(
  outdatedPackages: readonly OutdatedPackage[],
  opts: { long?: boolean }
): string {
  if (outdatedPackages.length === 0) {
    return ''
  }

  const columnNames = ['Package', 'Current', 'Latest']

  const columnFns = [renderPackageName, renderCurrent, renderLatest]

  if (opts.long) {
    columnNames.push('Details')
    columnFns.push(renderDetails)
  }

  // Avoid the overhead of allocating a new array caused by calling `array.map()`
  for (let i = 0; i < columnNames.length; i++) {
    columnNames[i] = chalk.blueBright(columnNames[i])
  }

  return table(
    [
      columnNames,
      ...sortOutdatedPackages(outdatedPackages).map((outdatedPkg) =>
        columnFns.map((fn) => fn(outdatedPkg))
      ),
    ],
    TABLE_OPTIONS
  )
}

function renderOutdatedList(
  outdatedPackages: readonly OutdatedPackage[],
  opts: { long?: boolean }
): string {
  if (outdatedPackages.length === 0) {
    return ''
  }

  return (
    sortOutdatedPackages(outdatedPackages)
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
  )
}

function renderOutdatedJSON(
  outdatedPackages: readonly OutdatedPackage[],
  opts: { long?: boolean }
): string {
  const outdatedPackagesJSON: Record<string, OutdatedPackageJSONOutput> =
    sortOutdatedPackages(outdatedPackages).reduce(
      (acc: Record<string, OutdatedPackageJSONOutput>, outdatedPkg): Record<string, OutdatedPackageJSONOutput> => {
        acc[outdatedPkg.packageName] = {
          current: outdatedPkg.current,
          latest: outdatedPkg.latestManifest?.version,
          wanted: outdatedPkg.wanted,
          isDeprecated: Boolean(outdatedPkg.latestManifest?.deprecated),
          dependencyType: outdatedPkg.belongsTo,
        }

        if (opts.long) {
          acc[outdatedPkg.packageName].latestManifest =
            outdatedPkg.latestManifest
        }

        return acc
      },
      {}
    )

  return JSON.stringify(outdatedPackagesJSON, null, 2)
}

function sortOutdatedPackages(outdatedPackages: readonly OutdatedPackage[]) {
  return sortWith(
    DEFAULT_COMPARATORS,
    outdatedPackages.map(toOutdatedWithVersionDiff)
  )
}

export function getCellWidth(
  data: string[][],
  columnNumber: number,
  maxWidth: number
) {
  const maxCellWidth = data.reduce((cellWidth, row) => {
    const cellLines = stripAnsi(row[columnNumber]).split('\n')
    const currentCellWidth = cellLines.reduce((lineWidth, line) => {
      return Math.max(lineWidth, line.length)
    }, 0)
    return Math.max(cellWidth, currentCellWidth)
  }, 0)
  return Math.min(maxWidth, maxCellWidth)
}

export function toOutdatedWithVersionDiff<T>(
  outdated: T & OutdatedPackage
): T & OutdatedWithVersionDiff {
  if (outdated.latestManifest != null) {
    return {
      ...outdated,
      ...semverDiff(outdated.wanted, outdated.latestManifest.version),
    }
  }
  return {
    ...outdated,
    change: 'unknown',
  }
}

export function renderPackageName({ belongsTo, packageName }: OutdatedPackage) {
  switch (belongsTo) {
    case 'devDependencies':
      return `${packageName} ${chalk.dim('(dev)')}`
    case 'optionalDependencies':
      return `${packageName} ${chalk.dim('(optional)')}`
    default:
      return packageName
  }
}

export function renderCurrent({ current, wanted }: OutdatedPackage) {
  const output = current ?? 'missing'
  if (current === wanted) return output
  return `${output} (wanted ${wanted})`
}

export function renderLatest(outdatedPkg: OutdatedWithVersionDiff): string {
  const { latestManifest, change, diff } = outdatedPkg
  if (latestManifest == null) return ''
  if (change === null || diff == null) {
    return latestManifest.deprecated
      ? chalk.redBright.bold('Deprecated')
      : latestManifest.version
  }

  return colorizeSemverDiff({ change, diff })
}

export function renderDetails({ latestManifest }: OutdatedPackage) {
  if (latestManifest == null) return ''
  const outputs = []
  if (latestManifest.deprecated) {
    outputs.push(wrapAnsi(chalk.redBright(latestManifest.deprecated), 40))
  }
  if (latestManifest.homepage) {
    outputs.push(chalk.underline(latestManifest.homepage))
  }
  return outputs.join('\n')
}
