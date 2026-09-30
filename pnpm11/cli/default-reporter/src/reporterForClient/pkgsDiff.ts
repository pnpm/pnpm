import type * as logs from '@pnpm/core-loggers'
import type { BaseManifest } from '@pnpm/types'
import { difference, mergeRight } from 'ramda'
import * as Rx from 'rxjs'
import { filter, map, mapTo, reduce, scan, startWith, take } from 'rxjs/operators'

export interface PackageDiff {
  added: boolean
  from?: string
  name: string
  realName?: string
  version?: string
  deprecated?: boolean
  latest?: string
}

export interface RecordByString<Value> {
  [index: string]: Value
}

export const propertyByDependencyType = {
  dev: 'devDependencies',
  nodeModulesOnly: 'node_modules',
  optional: 'optionalDependencies',
  peer: 'peerDependencies',
  prod: 'dependencies',
} as const

export interface PkgsDiff {
  dev: RecordByString<PackageDiff>
  nodeModulesOnly: RecordByString<PackageDiff>
  optional: RecordByString<PackageDiff>
  peer: RecordByString<PackageDiff>
  prod: RecordByString<PackageDiff>
}

export function getPkgsDiff (
  log$: {
    deprecation: Rx.Observable<logs.DeprecationLog>
    summary: Rx.Observable<logs.SummaryLog>
    root: Rx.Observable<logs.RootLog>
    packageManifest: Rx.Observable<logs.PackageManifestLog>
  },
  opts: {
    prefix?: string
  }
): Rx.Observable<PkgsDiff> {
  const deprecationSet$ = log$.deprecation
    .pipe(
      filter((log) => !opts.prefix || log.prefix === opts.prefix),
      scan((acc, log) => {
        acc.add(log.pkgId)
        return acc
      }, new Set<string>()),
      startWith(new Set<string>())
    )

  const filterPrefix = opts.prefix
    ? filter((log: { prefix: string }) => log.prefix === opts.prefix)
    : <Entry>(stream: Rx.Observable<Entry>) => stream
  const pkgsDiff$ = Rx.combineLatest(
    log$.root.pipe(filterPrefix) as Rx.Observable<logs.RootLog>,
    deprecationSet$
  ).pipe(
    scan(applyRootLog, createEmptyPkgsDiff()),
    startWith(createEmptyPkgsDiff())
  )

  const packageManifest$ = Rx.merge(
    log$.packageManifest.pipe(filterPrefix),
    log$.summary.pipe(filterPrefix, mapTo({}))
  )
    .pipe(
      take(2),
      reduce(mergeRight, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any -- the result is cast to PackageManifestLog right below
    ) as Rx.Observable<logs.PackageManifestLog>

  return Rx.combineLatest(
    pkgsDiff$,
    packageManifest$
  )
    .pipe(
      map(([pkgsDiff, packageManifests]) => addDirectDepsChanges(pkgsDiff, packageManifests))
    )
}

function createEmptyPkgsDiff (): PkgsDiff {
  return {
    dev: {},
    nodeModulesOnly: {},
    optional: {},
    peer: {},
    prod: {},
  }
}

interface RootDepChange {
  id?: string
  name: string
  realName?: string
  version?: string
  dependencyType?: string
  latest?: string
  linkedFrom?: string
}

function applyRootLog (pkgsDiff: PkgsDiff, [rootLog, deprecationSet]: [logs.RootLog, Set<string>]): PkgsDiff {
  const change = getRootDepChange(rootLog)
  if (change == null) return pkgsDiff
  const { action, log } = change
  const depType = (log.dependencyType || 'nodeModulesOnly') as keyof PkgsDiff
  const oppositeKey = `${action === '-' ? '+' : '-'}${log.name}`
  const previous = pkgsDiff[depType][oppositeKey]
  if (previous && previous.version === log.version) {
    delete pkgsDiff[depType][oppositeKey]
    return pkgsDiff
  }
  pkgsDiff[depType][`${action}${log.name}`] = {
    added: action === '+',
    deprecated: deprecationSet.has(log.id!),
    from: log.linkedFrom,
    latest: log.latest,
    name: log.name,
    realName: log.realName,
    version: log.version ?? log.id,
  }
  return pkgsDiff
}

function getRootDepChange (rootLog: logs.RootLog): { action: '-' | '+', log: RootDepChange } | undefined {
  if ('added' in rootLog) return { action: '+', log: rootLog['added'] }
  if ('removed' in rootLog) return { action: '-', log: rootLog['removed'] }
  return undefined
}

function addDirectDepsChanges (pkgsDiff: PkgsDiff, packageManifests: logs.PackageManifestLog): PkgsDiff {
  if ((packageManifests['initial'] == null) || (packageManifests['updated'] == null)) return pkgsDiff

  const initialPackageManifest = removeOptionalFromProdDeps(packageManifests['initial'])
  const updatedPackageManifest = removeOptionalFromProdDeps(packageManifests['updated'])

  for (const depType of ['peer', 'prod', 'optional', 'dev'] as const) {
    const prop = propertyByDependencyType[depType]
    const initialDeps = initialPackageManifest[prop] ?? {}
    const updatedDeps = updatedPackageManifest[prop] ?? {}
    addMissingDepChanges(pkgsDiff[depType], { from: initialDeps, to: updatedDeps, added: false })
    addMissingDepChanges(pkgsDiff[depType], { from: updatedDeps, to: initialDeps, added: true })
  }
  return pkgsDiff
}

/**
 * Records the dependencies of `opts.from` that `opts.to` lacks, unless a
 * change was already recorded for them.
 */
function addMissingDepChanges (
  diff: RecordByString<PackageDiff>,
  opts: { from: Record<string, string>, to: Record<string, string>, added: boolean }
): void {
  const action = opts.added ? '+' : '-'
  for (const depName of difference(Object.keys(opts.from), Object.keys(opts.to))) {
    if (!diff[`${action}${depName}`]) {
      diff[`${action}${depName}`] = {
        added: opts.added,
        name: depName,
        version: opts.from[depName],
      }
    }
  }
}

function removeOptionalFromProdDeps<Pkg extends BaseManifest> (pkg: Pkg): Pkg {
  if ((pkg.dependencies == null) || (pkg.optionalDependencies == null)) return pkg
  for (const depName of Object.keys(pkg.dependencies)) {
    if (pkg.optionalDependencies[depName]) {
      delete pkg.dependencies[depName]
    }
  }
  return pkg
}
