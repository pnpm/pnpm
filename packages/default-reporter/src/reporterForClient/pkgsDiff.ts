import * as logs from '@pnpm/core-loggers'
import { PackageManifest } from '@pnpm/types'
import * as Rx from 'rxjs'
import { filter, map, mapTo, reduce, scan, startWith, take } from 'rxjs/operators'
import R = require('ramda')

export interface PackageDiff {
  added: boolean
  from?: string
  name: string
  realName?: string
  version?: string
  deprecated?: boolean
  latest?: string
}

export interface Map<T> {
  [index: string]: T
}

export const propertyByDependencyType = {
  dev: 'devDependencies',
  nodeModulesOnly: 'node_modules',
  optional: 'optionalDependencies',
  peer: 'peerDependencies',
  prod: 'dependencies',
}

export default function (
  log$: {
    deprecation: Rx.Observable<logs.DeprecationLog>
    summary: Rx.Observable<logs.SummaryLog>
    root: Rx.Observable<logs.RootLog>
    packageManifest: Rx.Observable<logs.PackageManifestLog>
  },
  opts: {
    prefix: string
  }
) {
  const deprecationSet$ = log$.deprecation
    .pipe(
      filter((log) => log.prefix === opts.prefix),
      scan((acc, log) => {
        acc.add(log.pkgId)
        return acc
      }, new Set()),
      startWith(new Set())
    )

  const filterPrefix = filter((log: { prefix: string }) => log.prefix === opts.prefix)
  const pkgsDiff$ = Rx.combineLatest(
    log$.root.pipe(filterPrefix),
    deprecationSet$
  ).pipe(
    scan((pkgsDiff, args) => {
      const rootLog = args[0]
      const deprecationSet = args[1] as Set<string>
      if (rootLog['added']) {
        pkgsDiff[rootLog['added'].dependencyType || 'nodeModulesOnly'][`+${rootLog['added'].name as string}`] = {
          added: true,
          deprecated: deprecationSet.has(rootLog['added'].id),
          from: rootLog['added'].linkedFrom,
          latest: rootLog['added'].latest,
          name: rootLog['added'].name,
          realName: rootLog['added'].realName,
          version: rootLog['added'].version,
        }
        return pkgsDiff
      }
      if (rootLog['removed']) {
        pkgsDiff[rootLog['removed'].dependencyType || 'nodeModulesOnly'][`-${rootLog['removed'].name as string}`] = {
          added: false,
          name: rootLog['removed'].name,
          version: rootLog['removed'].version,
        }
        return pkgsDiff
      }
      return pkgsDiff
    }, {
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
    }),
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
  )
    .pipe(
      take(2),
      reduce(R.merge, {} as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    )

  return Rx.combineLatest(
    pkgsDiff$,
    packageManifest$
  )
    .pipe(
      map(
        ([pkgsDiff, packageManifests]: [
          {
            dev: Map<PackageDiff>
            nodeModulesOnly: Map<PackageDiff>
            optional: Map<PackageDiff>
            prod: Map<PackageDiff>
          },
          {
            initial?: PackageManifest
            updated?: PackageManifest
          }
        ]) => {
          if (!packageManifests['initial'] || !packageManifests['updated']) return pkgsDiff

          const initialPackageManifest = removeOptionalFromProdDeps(packageManifests['initial'])
          const updatedPackageManifest = removeOptionalFromProdDeps(packageManifests['updated'])

          for (const depType of ['peer', 'prod', 'optional', 'dev']) {
            const prop = propertyByDependencyType[depType]
            const initialDeps = Object.keys(initialPackageManifest[prop] || {})
            const updatedDeps = Object.keys(updatedPackageManifest[prop] || {})
            const removedDeps = R.difference(initialDeps, updatedDeps)

            for (const removedDep of removedDeps) {
              if (!pkgsDiff[depType][`-${removedDep}`]) {
                pkgsDiff[depType][`-${removedDep}`] = {
                  added: false,
                  name: removedDep,
                  version: initialPackageManifest[prop][removedDep],
                }
              }
            }

            const addedDeps = R.difference(updatedDeps, initialDeps)

            for (const addedDep of addedDeps) {
              if (!pkgsDiff[depType][`+${addedDep}`]) {
                pkgsDiff[depType][`+${addedDep}`] = {
                  added: true,
                  name: addedDep,
                  version: updatedPackageManifest[prop][addedDep],
                }
              }
            }
          }
          return pkgsDiff
        }
      )
    )
}

function removeOptionalFromProdDeps (pkg: PackageManifest): PackageManifest {
  if (!pkg.dependencies || !pkg.optionalDependencies) return pkg
  for (const depName of Object.keys(pkg.dependencies)) {
    if (pkg.optionalDependencies[depName]) {
      delete pkg.dependencies[depName]
    }
  }
  return pkg
}
