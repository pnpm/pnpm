import PnpmError from '@pnpm/error'
import {
  getLockfileImporterId,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import outdated, {
  forPackages as outdatedForPackages, OutdatedPackage,
} from '@pnpm/outdated'
import semverDiff, { SEMVER_CHANGE } from '@pnpm/semver-diff'
import storePath from '@pnpm/store-path'
import { PackageJson, Registries } from '@pnpm/types'
import chalk from 'chalk'
import R = require('ramda')
import { table } from 'table'
import wrapAnsi = require('wrap-ansi')
import createLatestManifestGetter from '../createLatestManifestGetter'
import { readImporterManifestOnly } from '../readImporterManifest'

export type OutdatedWithVersionDiff = OutdatedPackage & { change: SEMVER_CHANGE | null, diff?: [string[], string[]] }

export const TABLE_OPTIONS = {
  border: {
    bodyLeft: '',
    bottomLeft: '',
    joinLeft: '',
    topLeft: '',
  },
  columns: {
    0: {
      paddingLeft: 0,
    },
  },
}

export default async function (
  args: string[],
  opts: {
    alwaysAuth: boolean,
    ca?: string,
    cert?: string,
    engineStrict?: boolean,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMaxtimeout: number,
    fetchRetryMintimeout: number,
    global: boolean,
    httpsProxy?: string,
    independentLeaves: boolean,
    key?: string,
    localAddress?: string,
    networkConcurrency: number,
    offline: boolean,
    prefix: string,
    proxy?: string,
    rawNpmConfig: object,
    registries: Registries,
    lockfileDirectory?: string,
    store?: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
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

  const columnNames = [
    'Package',
    'Current',
    'Latest',
    'Details',
  ]
  let columnFns: Array<(outdatedPkg: OutdatedWithVersionDiff) => string> = [
    renderPackageName,
    renderCurrent,
    renderLatest,
    renderDetails,
  ]
  return table([
    columnNames,
    ...R.sortWith(
      [
        sortBySemverChange,
        (o1, o2) => o1.packageName.localeCompare(o2.packageName),
      ],
      outdatedPackages.map(toOutdatedWithVersionDiff)
    )
      .map((outdatedPkg) => columnFns.map((fn) => fn(outdatedPkg))),
  ], TABLE_OPTIONS)
}

export function getCellWidth (data: string[][], columnNumber: number, maxWidth: number) {
  return Math.min(maxWidth, data.reduce((maxWidth, row) => Math.max(maxWidth, row[columnNumber].length), 0))
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

export function renderPackageName ({ belongsTo, packageName }: OutdatedWithVersionDiff) {
  switch (belongsTo) {
    case 'devDependencies': return `${packageName} ${chalk.dim('(dev)')}`
    case 'optionalDependencies': return `${packageName} ${chalk.dim('(optional)')}`
    default: return packageName
  }
}

export function renderCurrent ({ current, wanted }: OutdatedWithVersionDiff) {
  let output = current || 'missing'
  if (current === wanted) return output
  return `${output} (wanted ${wanted})`
}

const DIFF_COLORS = {
  feature: chalk.yellowBright.bold,
  fix: chalk.greenBright.bold,
}

export function renderLatest ({ latestManifest, change, diff }: OutdatedWithVersionDiff) {
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

export function renderDetails ({ latestManifest }: OutdatedWithVersionDiff) {
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
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  opts: {
    alwaysAuth: boolean,
    ca?: string,
    cert?: string,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMaxtimeout: number,
    fetchRetryMintimeout: number,
    global: boolean,
    httpsProxy?: string,
    independentLeaves: boolean,
    key?: string,
    localAddress?: string,
    networkConcurrency: number,
    offline: boolean,
    prefix: string,
    proxy?: string,
    rawNpmConfig: object,
    registries: Registries,
    lockfileDirectory?: string,
    store?: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
) {
  const lockfileDirectory = opts.lockfileDirectory || opts.prefix
  const currentLockfile = await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false })
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
    const optsForOutdated = {
      currentLockfile,
      getLatestManifest,
      lockfileDirectory,
      manifest,
      prefix: path,
      wantedLockfile,
    }
    return {
      manifest,
      outdatedPackages: args.length
        ? await outdatedForPackages(args, optsForOutdated)
        : await outdated(optsForOutdated),
      prefix: getLockfileImporterId(lockfileDirectory, path),
    }
  }))
}
