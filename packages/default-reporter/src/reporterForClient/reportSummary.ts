import {
  DeprecationLog,
  PackageManifestLog,
  RootLog,
  SummaryLog,
} from '@pnpm/core-loggers'
import { Config } from '@pnpm/config'
import * as Rx from 'rxjs'
import { map, take } from 'rxjs/operators'
import { EOL } from '../constants'
import getPkgsDiff, {
  PackageDiff,
  propertyByDependencyType,
} from './pkgsDiff'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants'
import chalk = require('chalk')
import path = require('path')
import R = require('ramda')
import semver = require('semver')

export default (
  log$: {
    deprecation: Rx.Observable<DeprecationLog>
    summary: Rx.Observable<SummaryLog>
    root: Rx.Observable<RootLog>
    packageManifest: Rx.Observable<PackageManifestLog>
  },
  opts: {
    cwd: string
    pnpmConfig?: Config
  }
) => {
  const pkgsDiff$ = getPkgsDiff(log$, { prefix: opts.cwd })

  const summaryLog$ = log$.summary.pipe(take(1))

  return Rx.combineLatest(
    pkgsDiff$,
    summaryLog$
  )
    .pipe(
      take(1),
      map(([pkgsDiff]) => {
        let msg = ''
        for (const depType of ['prod', 'optional', 'peer', 'dev', 'nodeModulesOnly']) {
          const diffs = R.values(pkgsDiff[depType])
          if (diffs.length) {
            msg += EOL
            if (opts.pnpmConfig?.global) {
              msg += chalk.cyanBright(`${opts.cwd}:`)
            } else {
              msg += chalk.cyanBright(`${propertyByDependencyType[depType] as string}:`)
            }
            msg += EOL
            msg += printDiffs(diffs, { prefix: opts.cwd })
            msg += EOL
          }
        }
        return Rx.of({ msg })
      })
    )
}

function printDiffs (
  pkgsDiff: PackageDiff[],
  opts: {
    prefix: string
  }
) {
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
    return result
  }).join(EOL)
  return msg
}
