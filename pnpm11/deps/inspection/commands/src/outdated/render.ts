import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { TABLE_OPTIONS } from '@pnpm/cli.utils'
import { colorizeSemverDiff } from '@pnpm/colorize-semver-diff'
import type { findOutdatedGitHubActions } from '@pnpm/deps.github-actions'
import type { OutdatedPackage } from '@pnpm/deps.inspection.outdated'
import { PnpmError } from '@pnpm/error'
import { semverDiff } from '@pnpm/semver-diff'
import { sanitizeInline } from '@pnpm/text.sanitize'
import type { DependenciesOrPeersField, PackageManifest } from '@pnpm/types'
import { table } from '@zkochan/table'
import chalk from 'chalk'
import { countBy, sortWith } from 'ramda'

import {
  DEFAULT_COMPARATORS,
  NAME_COMPARATOR,
  type OutdatedWithVersionDiff,
} from './utils.js'

export type OutdatedFormat = 'table' | 'list' | 'json'

export function selectOutdatedRenderer<Renderer> (
  format: OutdatedFormat | undefined,
  renderers: Record<OutdatedFormat, Renderer>
): Renderer {
  const selectedFormat: string = format ?? 'table'
  switch (selectedFormat) {
    case 'table': return renderers.table
    case 'list': return renderers.list
    case 'json': return renderers.json
    default: {
      throw new PnpmError('BAD_OUTDATED_FORMAT', `Unsupported format: ${format?.toString() ?? 'undefined'}`)
    }
  }
}

export type OutdatedItem = OutdatedPackage & { dependencyType?: 'githubAction' }

export function renderOutdatedTable (outdatedPackages: readonly OutdatedItem[], opts: { long?: boolean, sortBy?: 'name' }): string {
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
  for (let columnIndex = 0; columnIndex < columnNames.length; columnIndex++)
    columnNames[columnIndex] = chalk.blueBright(columnNames[columnIndex])

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

export function renderOutdatedList (outdatedPackages: readonly OutdatedItem[], opts: { long?: boolean, sortBy?: 'name' }): string {
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
  dependencyType: DependenciesOrPeersField | 'githubAction'
  latestManifest?: PackageManifest
}

export function renderOutdatedJSON (outdatedPackages: readonly OutdatedItem[], opts: { long?: boolean, sortBy?: 'name' }): string {
  const getOutdatedJSONKey = createOutdatedJSONKeyGetter(outdatedPackages)
  const outdatedPackagesJSON: Record<string, OutdatedPackageJSONOutput> = sortOutdatedPackages(outdatedPackages, { sortBy: opts.sortBy })
    .reduce((acc, outdatedPkg) => {
      const key = getOutdatedJSONKey(outdatedPkg)
      acc[key] = {
        current: outdatedPkg.current,
        latest: outdatedPkg.latestManifest?.version,
        wanted: outdatedPkg.wanted,
        isDeprecated: Boolean(outdatedPkg.latestManifest?.deprecated),
        dependencyType: outdatedPkg.dependencyType ?? outdatedPkg.belongsTo,
      }
      if (opts.long) {
        acc[key].latestManifest = outdatedPkg.latestManifest
      }
      return acc
    }, {} as Record<string, OutdatedPackageJSONOutput>)
  return JSON.stringify(outdatedPackagesJSON, null, 2)
}

function sortOutdatedPackages (outdatedPackages: readonly OutdatedItem[], opts?: { sortBy?: 'name' }) {
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
      ...semverDiff(outdated.wanted, outdated.latestManifest.version),
    }
  }
  return {
    ...outdated,
    change: 'unknown',
  }
}

export function renderPackageName (outdatedPkg: OutdatedItem): string {
  const label = getDependencyTypeLabel(outdatedPkg)
  return label == null ? outdatedPkg.packageName : `${outdatedPkg.packageName} ${chalk.dim(`(${label})`)}`
}

/**
 * JSON output is keyed by package name. A name that occurs more than once in
 * the report, such as one package installed at several versions across a
 * workspace, is keyed by name, current version, and dependency type instead,
 * so that no entry overwrites another.
 */
export function createOutdatedJSONKeyGetter (outdatedPackages: readonly OutdatedItem[]): (outdatedPkg: OutdatedItem) => string {
  const packageCounts = countBy((outdatedPkg) => outdatedPkg.packageName, outdatedPackages)
  return (outdatedPkg) => {
    if (packageCounts[outdatedPkg.packageName] === 1) return outdatedPkg.packageName
    const label = getDependencyTypeLabel(outdatedPkg)
    const suffix = label == null ? '' : ` (${label})`
    return outdatedPkg.current
      ? `${outdatedPkg.packageName}@${outdatedPkg.current}${suffix}`
      : `${outdatedPkg.packageName}${suffix}`
  }
}

function getDependencyTypeLabel ({ belongsTo, dependencyType }: OutdatedItem): string | undefined {
  if (dependencyType === 'githubAction') return 'github action'
  switch (belongsTo) {
    case 'devDependencies': return 'dev'
    case 'optionalDependencies': return 'optional'
    case 'peerDependencies': return 'peer'
    default: return undefined
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

  const versionText = colorizeSemverDiff({ change, diff })
  if (latestManifest.deprecated) {
    return `${versionText} ${chalk.redBright('(deprecated)')}`
  }

  return versionText
}

export function renderDetails ({ latestManifest }: OutdatedPackage): string {
  if (latestManifest == null) return ''
  const outputs = []
  if (latestManifest.deprecated) {
    outputs.push(chalk.redBright(sanitizeInline(latestManifest.deprecated)))
  }
  if (latestManifest.homepage) {
    outputs.push(chalk.underline(latestManifest.homepage))
  }
  return outputs.join('\n')
}

export function toOutdatedAction (action: Awaited<ReturnType<typeof findOutdatedGitHubActions>>[number]): OutdatedItem {
  return {
    alias: action.name,
    belongsTo: 'devDependencies',
    current: action.current,
    dependencyType: 'githubAction',
    latestManifest: {
      name: action.name,
      version: action.latest,
      homepage: action.homepage,
    },
    packageName: action.name,
    wanted: action.wanted,
  }
}
