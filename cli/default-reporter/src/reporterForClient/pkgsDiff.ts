import {
  filter,
  map,
  mapTo,
  reduce,
  scan,
  startWith,
  take,
} from 'rxjs/operators'
import * as Rx from 'rxjs'
import mergeRight from 'ramda/src/mergeRight'
import difference from 'ramda/src/difference'

import type { DeprecationLog, PackageManifestLog, RootLog, SummaryLog, PackageManifest, PackageDiff } from '@pnpm/types'

export interface Map<T> {
  [index: string]: T
}

export const propertyByDependencyType = {
  dev: 'devDependencies',
  nodeModulesOnly: 'node_modules',
  optional: 'optionalDependencies',
  peer: 'peerDependencies',
  prod: 'dependencies',
} as const

export function getPkgsDiff(
  log$: {
    deprecation: Rx.Observable<DeprecationLog>
    summary: Rx.Observable<SummaryLog>
    root: Rx.Observable<RootLog>
    packageManifest: Rx.Observable<PackageManifestLog>
  },
  opts: {
    prefix: string
  }
): Rx.Observable<{
  dev: Map<PackageDiff>;
  nodeModulesOnly: Map<PackageDiff>;
  optional: Map<PackageDiff>;
  prod: Map<PackageDiff>;
} | {
  dev: Map<PackageDiff>
  nodeModulesOnly: Map<PackageDiff>
  optional: Map<PackageDiff>
  prod: Map<PackageDiff>
}> {
  const deprecationSet$ = log$.deprecation.pipe(
    filter((log: DeprecationLog): boolean => {
      return log.prefix === opts.prefix;
    }),
    scan((acc: Set<string>, log: DeprecationLog): Set<string> => {
      acc.add(log.pkgId)
      return acc
    }, new Set()),
    startWith(new Set<string>())
  )

  const filterPrefix = filter(
    (log: { prefix: string }): boolean => {
      return log.prefix === opts.prefix;
    }
  )

  const pkgsDiff$ = Rx.combineLatest(
    log$.root.pipe(filterPrefix),
    deprecationSet$
  ).pipe(
    scan(
      (pkgsDiff, args) => {
        const rootLog = args[0]

        const deprecationSet = args[1] as Set<string>

        let action: '-' | '+' | undefined

        let log: any // eslint-disable-line @typescript-eslint/no-explicit-any

        if ('added' in rootLog) {
          action = '+'
          log = rootLog.added
        } else if ('removed' in rootLog) {
          action = '-'
          log = rootLog.removed
        } else {
          return pkgsDiff
        }

        const depType = (log.dependencyType ||
          'nodeModulesOnly') as keyof typeof pkgsDiff

        const oppositeKey = `${action === '-' ? '+' : '-'}${log.name}`

        const previous = pkgsDiff[depType][oppositeKey]

        if (previous && previous.version === log.version) {
          delete pkgsDiff[depType][oppositeKey]

          return pkgsDiff
        }

        pkgsDiff[depType][`${action}${log.name as string}`] = {
          added: action === '+',
          deprecated: deprecationSet.has(log.id),
          from: log.linkedFrom,
          latest: log.latest,
          name: log.name,
          realName: log.realName,
          version: log.version,
        }

        return pkgsDiff
      },
      {
        dev: {},
        nodeModulesOnly: {},
        optional: {},
        peer: {},
        prod: {},
      } as {
        dev: Map<PackageDiff>
        nodeModulesOnly: Map<PackageDiff>
        optional: Map<PackageDiff>
        prod: Map<PackageDiff>
      }
    ),
    startWith({
      dev: {},
      nodeModulesOnly: {},
      optional: {},
      peer: {},
      prod: {},
    })
  )

  const packageManifest$ = Rx.merge(
    log$.packageManifest.pipe(filterPrefix),
    log$.summary.pipe(filterPrefix, mapTo({}))
  ).pipe(
    take(2),
    reduce(mergeRight, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  )

  return Rx.combineLatest(pkgsDiff$, packageManifest$).pipe(
    map(([pkgsDiff, packageManifests]) => {
      // @ts-ignore
      if (packageManifests.initial == null || packageManifests.updated == null)
        return pkgsDiff

      const initialPackageManifest = removeOptionalFromProdDeps(
        // @ts-ignore
        packageManifests.initial
      )
      const updatedPackageManifest = removeOptionalFromProdDeps(
        // @ts-ignore
        packageManifests.updated
      )

      for (const depType of ['peer', 'prod', 'optional', 'dev'] as const) {
        const prop = propertyByDependencyType[depType]
        const initialDeps = Object.keys(initialPackageManifest[prop] || {})
        const updatedDeps = Object.keys(updatedPackageManifest[prop] || {})
        const removedDeps = difference(initialDeps, updatedDeps)

        for (const removedDep of removedDeps) {
          if (!pkgsDiff[depType][`-${removedDep}`]) {
            pkgsDiff[depType][`-${removedDep}`] = {
              added: false,
              name: removedDep,
              version: initialPackageManifest[prop]?.[removedDep],
            }
          }
        }

        const addedDeps = difference(updatedDeps, initialDeps)

        for (const addedDep of addedDeps) {
          if (!pkgsDiff[depType][`+${addedDep}`]) {
            pkgsDiff[depType][`+${addedDep}`] = {
              added: true,
              name: addedDep,
              version: updatedPackageManifest[prop]?.[addedDep],
            }
          }
        }
      }
      return pkgsDiff
    })
  )
}

function removeOptionalFromProdDeps(pkg: PackageManifest): PackageManifest {
  if (pkg.dependencies == null || pkg.optionalDependencies == null) {
    return pkg
  }

  for (const depName of Object.keys(pkg.dependencies)) {
    if (pkg.optionalDependencies[depName]) {
      delete pkg.dependencies[depName]
    }
  }
  return pkg
}
