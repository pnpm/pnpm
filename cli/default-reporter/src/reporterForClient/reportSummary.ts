import path from 'path'
import {
  type IgnoredScriptsLog,
  type DeprecationLog,
  type PackageManifestLog,
  type RootLog,
  type SummaryLog,
} from '@pnpm/core-loggers'
import { type Config } from '@pnpm/config'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'
import chalk from 'chalk'
import semver from 'semver'
import { EOL } from '../constants'
import {
  getPkgsDiff,
  type PackageDiff,
  propertyByDependencyType,
} from './pkgsDiff'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants'

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
    ignoredScripts: Rx.Observable<IgnoredScriptsLog>
  },
  opts: {
    cmd: string
    cwd: string
    env: NodeJS.ProcessEnv
    filterPkgsDiff?: FilterPkgsDiff
    pnpmConfig?: Config
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const pkgsDiff$ = getPkgsDiff(log$, { prefix: opts.cwd })

  const summaryLog$ = log$.summary.pipe(take(1))
  const _printDiffs = printDiffs.bind(null, { cmd: opts.cmd, prefix: opts.cwd, pnpmConfig: opts.pnpmConfig })

  return Rx.combineLatest(
    pkgsDiff$,
    log$.ignoredScripts.pipe(Rx.startWith({ packageNames: undefined })),
    summaryLog$
  )
    .pipe(
      take(1),
      map(([pkgsDiff, ignoredScripts]) => {
        let msg = ''
        for (const depType of ['prod', 'optional', 'peer', 'dev', 'nodeModulesOnly'] as const) {
          let diffs: PackageDiff[] = Object.values(pkgsDiff[depType as keyof typeof pkgsDiff])
          if (opts.filterPkgsDiff) {
            // This filtering is only used by Bit CLI currently.
            // Related PR: https://github.com/teambit/bit/pull/7176
            diffs = diffs.filter((pkgDiff) => opts.filterPkgsDiff!(pkgDiff))
          }
          if (diffs.length > 0) {
            msg += EOL
            if (opts.pnpmConfig?.global) {
              msg += chalk.cyanBright(`${opts.cwd}:`)
            } else {
              msg += chalk.cyanBright(`${propertyByDependencyType[depType] as string}:`)
            }
            msg += EOL
            msg += _printDiffs(diffs, depType)
            msg += EOL
          } else if (CONFIG_BY_DEP_TYPE[depType] && opts.pnpmConfig?.[CONFIG_BY_DEP_TYPE[depType]] === false) {
            msg += EOL
            msg += `${chalk.cyanBright(`${propertyByDependencyType[depType] as string}:`)} skipped`
            msg += EOL
          }
        }
        if (ignoredScripts.packageNames && ignoredScripts.packageNames.length > 0) {
          msg += EOL
          msg += `The following dependencies have build scripts that were ignored: ${Array.from(ignoredScripts.packageNames).sort().join(', ')}`
          msg += EOL
          msg += 'To allow the execution of build scripts for these packages, add their names to "pnpm.onlyBuiltDependencies" in your package.json'
          msg += EOL
        }
        return Rx.of({ msg })
      })
    )
}

export type FilterPkgsDiff = (pkgsDiff: PackageDiff) => boolean

function printDiffs (
  opts: {
    cmd: string
    prefix: string
    pnpmConfig?: Config
  },
  pkgsDiff: PackageDiff[],
  depType: string
): string {
  // Sorts by alphabet then by removed/added
  // + ava 0.10.0
  // - chalk 1.0.0
  // + chalk 2.0.0
  pkgsDiff.sort((a, b) => (a.name.localeCompare(b.name) * 10 + (Number(!b.added) - Number(!a.added))))
  const msg = pkgsDiff.map((pkg) => {
    let result = pkg.added
      ? ADDED_CHAR
      : REMOVED_CHAR
    if (!pkg.realName || pkg.name === pkg.realName) {
      result += ` ${pkg.name}`
    } else {
      result += ` ${pkg.name} <- ${pkg.realName}`
    }
    if (pkg.version) {
      result += ` ${chalk.grey(pkg.version)}`
      if (pkg.latest && semver.lt(pkg.version, pkg.latest)) {
        result += ` ${chalk.grey(`(${pkg.latest} is available)`)}`
      }
    }
    if (pkg.deprecated) {
      result += ` ${chalk.red('deprecated')}`
    }
    if (pkg.from) {
      result += ` ${chalk.grey(`<- ${pkg.from && path.relative(opts.prefix, pkg.from) || '???'}`)}`
    }
    if (pkg.added && depType === 'dev' && opts.pnpmConfig?.saveDev === false && opts.cmd === 'add') {
      result += `${chalk.yellow(' already in devDependencies, was not moved to dependencies.')}`
    }
    return result
  }).join(EOL)
  return msg
}
