import { Config } from '@pnpm/config'
import {
  DeprecationLog,
  PackageManifestLog,
  RootLog,
  SummaryLog,
} from '@pnpm/core-loggers'
import { EOL } from '../constants'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants'
import getPkgsDiff, {
  PackageDiff,
  propertyByDependencyType,
} from './pkgsDiff'
import path = require('path')
import chalk = require('chalk')
import most = require('most')
import R = require('ramda')
import semver = require('semver')

export default (
  log$: {
    deprecation: most.Stream<DeprecationLog>
    summary: most.Stream<SummaryLog>
    root: most.Stream<RootLog>
    packageManifest: most.Stream<PackageManifestLog>
  },
  opts: {
    cwd: string
    pnpmConfig?: Config
  }
) => {
  const pkgsDiff$ = getPkgsDiff(log$, { prefix: opts.cwd })

  const summaryLog$ = log$.summary
    .take(1)

  return most.combine(
    (pkgsDiff) => {
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
      return { msg }
    },
    pkgsDiff$,
    summaryLog$
  )
    .take(1)
    .map(most.of)
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
