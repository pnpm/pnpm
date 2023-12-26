import path from 'path'
import {
  type DeprecationLog,
  type PackageManifestLog,
  type RootLog,
  type SummaryLog,
} from '@pnpm/core-loggers'
import { type Config } from '@pnpm/config'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import chalk from 'chalk'
import semver from 'semver'
import { EOL } from '../constants'
import {
  getPkgsDiff,
  type PackageDiff,
  type PkgDiffs,
  propertyByDependencyType,
} from './pkgsDiff'
import {
  ADDED_CHAR,
  REMOVED_CHAR,
} from './outputConstants'

const CONFIG_BY_DEP_TYPE = {
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
    cwd: string
    env: NodeJS.ProcessEnv
    filterPkgsDiff?: FilterPkgsDiff
    pnpmConfig?: Config
  }
) {
  const pkgsDiff$ = getPkgsDiff(log$)

  const summaryLog$ = log$.summary
  const _printDiffs = printDiffs.bind(null, { prefix: opts.cwd })

  return Rx.combineLatest(
    pkgsDiff$,
    summaryLog$
  )
    .pipe(
      map(([pkgsDiff]) => {
        const pkgDiffsWithPrefix = groupPkgsDiffByPrefix(pkgsDiff)
        const isWorkspaces = opts.pnpmConfig?.workspaceDir
        const formattedMsg = Object.entries(pkgDiffsWithPrefix)
          .sort(([a], [b]) => a.length - b.length)
          .map(([prefix, _pkgsDiff]) => {
            let msg = ''
            for (const depType of ['prod', 'optional', 'peer', 'dev', 'nodeModulesOnly'] as const) {
              let diffs: PackageDiff[] = Object.values(_pkgsDiff[depType as keyof typeof _pkgsDiff])
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
                msg += _printDiffs(diffs)
                msg += EOL
              } else if (opts.pnpmConfig?.[CONFIG_BY_DEP_TYPE[depType]] === false) {
                msg += EOL
                msg += `${chalk.cyanBright(`${propertyByDependencyType[depType] as string}:`)} skipped`
                if (opts.env.NODE_ENV === 'production' && depType === 'dev') {
                  msg += ' because NODE_ENV is set to production'
                }
                msg += EOL
              }
            }
            const lockfileDir = opts.pnpmConfig?.lockfileDir ?? opts.pnpmConfig?.workspaceDir ?? opts.cwd
            const relativePath = path.relative(lockfileDir, prefix) || '.'
            // We only print prefix for pnpm workspaces
            if (msg.length && isWorkspaces) {
              msg = `${chalk.bold(chalk.cyanBright(relativePath))}${msg.split(EOL).map(segment => `\u0020\u0020${segment}`).join(EOL)}`
            }
            return msg
          }).join(EOL)
        return Rx.of({ msg: formattedMsg.length && isWorkspaces ? EOL + formattedMsg : formattedMsg })
      })
    )
}

export type FilterPkgsDiff = (pkgsDiff: PackageDiff) => boolean

function printDiffs (
  opts: {
    prefix: string
  },
  pkgsDiff: PackageDiff[]
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

function groupPkgsDiffByPrefix (pkgsDiff: PkgDiffs) {
  const pkgsDiffWithPrefix: Record<string, PkgDiffs> = {}
  for (const depType of ['prod', 'optional', 'peer', 'dev', 'nodeModulesOnly'] as const) {
    for (const [action, pkgDiff] of Object.entries(pkgsDiff[depType])) {
      if (!pkgsDiffWithPrefix[pkgDiff.prefix]) {
        pkgsDiffWithPrefix[pkgDiff.prefix] = {
          dev: {},
          nodeModulesOnly: {},
          optional: {},
          peer: {},
          prod: {},
        }
      }
      pkgsDiffWithPrefix[pkgDiff.prefix][depType][action] = pkgDiff
    }
  }
  return pkgsDiffWithPrefix
}
