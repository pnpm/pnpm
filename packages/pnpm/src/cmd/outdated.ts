import PnpmError from '@pnpm/error'
import {
  getLockfileImporterId,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import matcher from '@pnpm/matcher'
import { read as readModulesManifest } from '@pnpm/modules-yaml'
import outdated, { OutdatedPackage } from '@pnpm/outdated'
import semverDiff, { SEMVER_CHANGE } from '@pnpm/semver-diff'
import storePath from '@pnpm/store-path'
import { ImporterManifest, Registries } from '@pnpm/types'
import chalk from 'chalk'
import { stripIndent } from 'common-tags'
import path = require('path')
import R = require('ramda')
import stripAnsi from 'strip-ansi'
import { table } from 'table'
import wrapAnsi = require('wrap-ansi')
import createLatestManifestGetter from '../createLatestManifestGetter'
import { readImporterManifestOnly } from '../readImporterManifest'

export type OutdatedWithVersionDiff = OutdatedPackage & { change: SEMVER_CHANGE | null, diff?: [string[], string[]] }

export const TABLE_OPTIONS = {
  border: {
    topBody: '─',
    topJoin: '┬',
    topLeft: '┌',
    topRight: '┐',

    bottomBody: '─',
    bottomJoin: '┴',
    bottomLeft: '└',
    bottomRight: '┘',

    bodyJoin: '│',
    bodyLeft: '│',
    bodyRight: '│',

    joinBody: '─',
    joinJoin: '┼',
    joinLeft: '├',
    joinRight: '┤'
  },
  columns: {},
}

for (let [key, value] of Object.entries(TABLE_OPTIONS.border)) {
  TABLE_OPTIONS.border[key] = chalk.grey(value)
}

/**
 * Default comparators used as the argument to `ramda.sortWith()`.
 */
export const DEFAULT_COMPARATORS = [
  sortBySemverChange,
  (o1: OutdatedWithVersionDiff, o2: OutdatedWithVersionDiff) => o1.packageName.localeCompare(o2.packageName),
]

export interface OutdatedOptions {
  alwaysAuth: boolean
  ca?: string
  cert?: string
  engineStrict?: boolean
  fetchRetries: number
  fetchRetryFactor: number
  fetchRetryMaxtimeout: number
  fetchRetryMintimeout: number
  global: boolean
  httpsProxy?: string
  independentLeaves: boolean
  key?: string
  localAddress?: string
  long?: boolean
  networkConcurrency: number
  offline: boolean
  prefix: string
  proxy?: string
  rawConfig: object
  registries: Registries
  lockfileDirectory?: string
  store?: string
  strictSsl: boolean
  table?: boolean
  tag: string
  userAgent: string
}

export default async function (
  args: string[],
  opts: OutdatedOptions,
  command: string,
) {
  const packages = [
    {
      manifest: await readImporterManifestOnly(opts.prefix, opts),
      path: opts.prefix,
    },
  ]
  const { outdatedPackages } = (await outdatedDependenciesOfWorkspacePackages(packages, args, opts))[0]

  if (!outdatedPackages.length) return

  if (opts.table !== false) {
    return renderOutdatedTable(outdatedPackages, opts)
  } else {
    return renderOutdatedList(outdatedPackages, opts)
  }
}

function renderOutdatedTable (outdatedPackages: ReadonlyArray<OutdatedPackage>, opts: { long?: boolean }) {
  let columnNames = [
    'Package',
    'Current',
    'Latest'
  ]

  let columnFns = [
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

  return table([
    columnNames,
    ...sortOutdatedPackages(outdatedPackages)
      .map((outdatedPkg) => columnFns.map((fn) => fn(outdatedPkg))),
  ], TABLE_OPTIONS)
}

function renderOutdatedList (outdatedPackages: ReadonlyArray<OutdatedPackage>, opts: { long?: boolean }) {
  return sortOutdatedPackages(outdatedPackages)
    .map((outdatedPkg) => {
      let info = stripIndent`
        ${chalk.bold(renderPackageName(outdatedPkg))}
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

function sortOutdatedPackages (outdatedPackages: ReadonlyArray<OutdatedPackage>) {
  return R.sortWith(
    DEFAULT_COMPARATORS,
    outdatedPackages.map(toOutdatedWithVersionDiff),
  )
}

export function getCellWidth (data: string[][], columnNumber: number, maxWidth: number) {
  const maxCellWidth = data.reduce((cellWidth, row) => {
    const cellLines = stripAnsi(row[columnNumber]).split('\n')
    const currentCellWidth = cellLines.reduce((lineWidth, line) => {
      return Math.max(lineWidth, line.length)
    }, 0)
    return Math.max(cellWidth, currentCellWidth)
  }, 0)
  return Math.min(maxWidth, maxCellWidth)
}

export function toOutdatedWithVersionDiff<T> (outdated: T & OutdatedPackage): T & OutdatedWithVersionDiff {
  if (outdated.latestManifest) {
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

export function renderPackageName ({ belongsTo, packageName }: OutdatedPackage) {
  switch (belongsTo) {
    case 'devDependencies': return `${packageName} ${chalk.dim('(dev)')}`
    case 'optionalDependencies': return `${packageName} ${chalk.dim('(optional)')}`
    default: return packageName
  }
}

export function renderCurrent ({ current, wanted }: OutdatedPackage) {
  let output = current || 'missing'
  if (current === wanted) return output
  return `${output} (wanted ${wanted})`
}

const DIFF_COLORS = {
  feature: chalk.yellowBright.bold,
  fix: chalk.greenBright.bold,
}

export function renderLatest (outdatedPkg: OutdatedWithVersionDiff): string {
  const { latestManifest, change, diff } = outdatedPkg
  if (!latestManifest) return ''
  if (change === null || !diff) {
    return latestManifest.deprecated
      ? chalk.redBright.bold('Deprecated')
      : latestManifest.version
  }

  const highlight = DIFF_COLORS[change] || chalk.redBright.bold
  const same = joinVersionTuples(diff[0], 0)
  const other = highlight(joinVersionTuples(diff[1], diff[0].length))
  if (!same) return other
  if (!other) {
    // Happens when current is 1.0.0-rc.0 and latest is 1.0.0
    return same
  }
  return diff[0].length === 3 ? `${same}-${other}` : `${same}.${other}`
}

function joinVersionTuples (versionTuples: string[], startIndex: number) {
  const neededForSemver = 3 - startIndex
  if (versionTuples.length <= neededForSemver || neededForSemver === 0) {
    return versionTuples.join('.')
  }
  return `${
    versionTuples.slice(0, neededForSemver).join('.')
   }-${
     versionTuples.slice(neededForSemver).join('.')
   }`
}

export function sortBySemverChange (outdated1: OutdatedWithVersionDiff, outdated2: OutdatedWithVersionDiff) {
  return pkgPriority(outdated1) - pkgPriority(outdated2)
}

function pkgPriority (pkg: OutdatedWithVersionDiff) {
  switch (pkg.change) {
    case null: return 0
    case 'fix': return 1
    case 'feature': return 2
    case 'breaking': return 3
    default: return 4
  }
}

export function renderDetails ({ latestManifest }: OutdatedPackage) {
  if (!latestManifest) return ''
  const outputs = []
  if (latestManifest.deprecated) {
    outputs.push(wrapAnsi(chalk.redBright(latestManifest.deprecated), 40))
  }
  if (latestManifest.homepage) {
    outputs.push(chalk.underline(latestManifest.homepage))
  }
  return outputs.join('\n')
}

export async function outdatedDependenciesOfWorkspacePackages (
  pkgs: Array<{path: string, manifest: ImporterManifest}>,
  args: string[],
  opts: OutdatedOptions,
) {
  const lockfileDirectory = opts.lockfileDirectory || opts.prefix
  const modules = await readModulesManifest(path.join(lockfileDirectory, 'node_modules'))
  const virtualStoreDir = modules?.virtualStoreDir || path.join(lockfileDirectory, 'node_modules/.pnpm')
  const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDirectory, { ignoreIncompatible: false }) || currentLockfile
  if (!wantedLockfile) {
    throw new PnpmError('OUTDATED_NO_LOCKFILE', 'No lockfile in this directory. Run `pnpm install` to generate one.')
  }
  const store = await storePath(opts.prefix, opts.store)
  const getLatestManifest = createLatestManifestGetter({
    ...opts,
    lockfileDirectory,
    store,
  })
  return Promise.all(pkgs.map(async ({ manifest, path }) => {
    let match = args.length && matcher(args) || undefined
    return {
      manifest,
      outdatedPackages: await outdated({
        currentLockfile,
        getLatestManifest,
        lockfileDirectory,
        manifest,
        match,
        prefix: path,
        wantedLockfile,
      }),
      prefix: getLockfileImporterId(lockfileDirectory, path),
    }
  }))
}
