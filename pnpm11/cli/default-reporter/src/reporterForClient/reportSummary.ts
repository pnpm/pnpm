import path from 'node:path'

import type {
  DeprecationLog,
  PackageManifestLog,
  RootLog,
  SummaryLog,
} from '@pnpm/core-loggers'
import chalk from 'chalk'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'
import semver from 'semver'

import { EOL } from '../constants.js'
import type { ReporterPnpmConfig } from '../ReporterPnpmConfig.js'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants.js'
import {
  getPkgsDiff,
  type PackageDiff,
  type PkgsDiff,
  propertyByDependencyType,
} from './pkgsDiff.js'

type DepType = 'prod' | 'optional' | 'peer' | 'dev' | 'nodeModulesOnly'

type ConfigByDepType = 'production' | 'dev' | 'optional'

const CONFIG_BY_DEP_TYPE: Partial<Record<DepType, ConfigByDepType>> = {
  prod: 'production',
  dev: 'dev',
  optional: 'optional',
}

export function reportSummary (
  log$: {
    deprecation: Rx.Observable<DeprecationLog>
    summary: Rx.Observable<SummaryLog>
    root: Rx.Observable<RootLog>
    packageManifest: Rx.Observable<PackageManifestLog>
  },
  opts: {
    cmd: string
    cwd: string
    env: NodeJS.ProcessEnv
    filterPkgsDiff?: FilterPkgsDiff
    pnpmConfig?: ReporterPnpmConfig
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const pkgsDiff$ = getPkgsDiff(log$, { prefix: opts.pnpmConfig?.global ? undefined : opts.cwd })

  const summaryLog$ = log$.summary.pipe(take(1))
  const _printDiffs = printDiffs.bind(null, { cmd: opts.cmd, prefix: opts.cwd, pnpmConfig: opts.pnpmConfig })

  return Rx.combineLatest(
    pkgsDiff$,
    summaryLog$
  )
    .pipe(
      take(1),
      map(([pkgsDiff]) => {
        let msg = ''
        for (const depType of ['prod', 'optional', 'peer', 'dev', 'nodeModulesOnly'] as const) {
          msg += formatDepTypeSummary(pkgsDiff, depType, { ...opts, printDiffs: _printDiffs })
        }
        return Rx.of({ msg })
      })
    )
}

export type FilterPkgsDiff = (pkgsDiff: PackageDiff) => boolean

function formatDepTypeSummary (
  pkgsDiff: PkgsDiff,
  depType: DepType,
  opts: {
    filterPkgsDiff?: FilterPkgsDiff
    pnpmConfig?: ReporterPnpmConfig
    printDiffs: (pkgsDiff: PackageDiff[], depType: string) => string
  }
): string {
  let diffs: PackageDiff[] = Object.values(pkgsDiff[depType as keyof typeof pkgsDiff])
  if (opts.filterPkgsDiff) {
    // This filtering is only used by Bit CLI currently.
    // Related PR: https://github.com/teambit/bit/pull/7176
    diffs = diffs.filter((pkgDiff) => opts.filterPkgsDiff!(pkgDiff))
  }
  if (diffs.length > 0) {
    return EOL +
      chalk.cyanBright(opts.pnpmConfig?.global ? 'global:' : `${propertyByDependencyType[depType] as string}:`) +
      EOL +
      opts.printDiffs(diffs, depType) +
      EOL
  }
  if (CONFIG_BY_DEP_TYPE[depType] && opts.pnpmConfig?.[CONFIG_BY_DEP_TYPE[depType]] === false) {
    return EOL + `${chalk.cyanBright(`${propertyByDependencyType[depType] as string}:`)} skipped` + EOL
  }
  return ''
}

function printDiffs (
  opts: {
    cmd: string
    prefix: string
    pnpmConfig?: ReporterPnpmConfig
  },
  pkgsDiff: PackageDiff[],
  depType: string
): string {
  // Sorts by alphabet then by removed/added
  // + ava 0.10.0
  // - chalk 1.0.0
  // + chalk 2.0.0
  pkgsDiff.sort((a, b) => (a.name.localeCompare(b.name) * 10 + (Number(!b.added) - Number(!a.added))))
  const msg = pkgsDiff.map((pkg) => formatPkgDiff(pkg, depType, opts)).join(EOL)
  return msg
}

function formatPkgDiff (
  pkg: PackageDiff,
  depType: string,
  opts: {
    cmd: string
    prefix: string
    pnpmConfig?: ReporterPnpmConfig
  }
): string {
  let result = pkg.added
    ? ADDED_CHAR
    : REMOVED_CHAR
  result += formatPkgDiffName(pkg)
  if (pkg.version) {
    result += formatPkgDiffVersion(pkg.version, pkg.latest)
  }
  if (pkg.deprecated) {
    result += ` ${chalk.red('deprecated')}`
  }
  if (pkg.from) {
    result += ` ${chalk.grey(`<- ${path.relative(opts.prefix, pkg.from) || pkg.from}`)}`
  }
  if (pkg.added && depType === 'dev' && opts.pnpmConfig?.saveDev === false && opts.cmd === 'add') {
    result += `${chalk.yellow(' already in devDependencies, was not moved to dependencies.')}`
  }
  return result
}

function formatPkgDiffName (pkg: PackageDiff): string {
  if (!pkg.realName || pkg.name === pkg.realName) {
    return ` ${pkg.name}`
  }
  return ` ${pkg.name} <- ${pkg.realName}`
}

function formatPkgDiffVersion (version: string, latest: string | undefined): string {
  let result = ` ${chalk.grey(version)}`
  if (latest && semver.lt(version, latest)) {
    result += ` ${chalk.grey(`(${latest} is available)`)}`
  }
  return result
}
