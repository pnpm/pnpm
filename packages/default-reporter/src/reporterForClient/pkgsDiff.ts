import * as logs from '@pnpm/core-loggers'
import { PackageManifest } from '@pnpm/types'
import most = require('most')
import R = require('ramda')

export interface PackageDiff {
  added: boolean,
  from?: string,
  name: string,
  realName?: string,
  version?: string,
  deprecated?: boolean,
  latest?: string,
}

export interface Map<T> {
  [index: string]: T,
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
    deprecation: most.Stream<logs.DeprecationLog>,
    summary: most.Stream<logs.SummaryLog>,
    root: most.Stream<logs.RootLog>,
    packageManifest: most.Stream<logs.PackageManifestLog>,
  },
  opts: {
    prefix: string,
  }
) {
  const deprecationSet$ = log$.deprecation
    .filter((log) => log.prefix === opts.prefix)
    .scan((acc, log) => {
      acc.add(log.pkgId)
      return acc
    }, new Set())

  const pkgsDiff$ = most.combine(
    (rootLog, deprecationSet) => [rootLog, deprecationSet],
    log$.root.filter((log) => log.prefix === opts.prefix),
    deprecationSet$
  )
  .scan((pkgsDiff, args) => {
    const rootLog = args[0]
    const deprecationSet = args[1] as Set<string>
    if (rootLog['added']) {
      pkgsDiff[rootLog['added'].dependencyType || 'nodeModulesOnly'][`+${rootLog['added'].name}`] = {
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
      pkgsDiff[rootLog['removed'].dependencyType || 'nodeModulesOnly'][`-${rootLog['removed'].name}`] = {
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
    dev: Map<PackageDiff>,
    nodeModulesOnly: Map<PackageDiff>,
    optional: Map<PackageDiff>,
    prod: Map<PackageDiff>,
  })

  const packageManifest$ = most.fromPromise(
    most.merge(
      log$.packageManifest.filter((log) => log.prefix === opts.prefix),
      log$.summary.filter((log) => log.prefix === opts.prefix).constant({})
    )
    .take(2)
    .reduce(R.merge, {} as any) // tslint:disable-line:no-any
  )

  return most.combine(
    (pkgsDiff, packageManifests: { initial?: PackageManifest, updated?: PackageManifest }) => {
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
    },
    pkgsDiff$,
    packageManifest$
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
