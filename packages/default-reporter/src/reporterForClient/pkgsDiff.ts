import * as logs from '@pnpm/core-loggers'
import { PackageJson } from '@pnpm/types'
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
  prod: 'dependencies',
}

export default function (
  log$: {
    deprecation: most.Stream<logs.DeprecationLog>,
    summary: most.Stream<logs.SummaryLog>,
    root: most.Stream<logs.RootLog>,
    packageJson: most.Stream<logs.PackageJsonLog>,
  },
  opts: {
    prefix: string,
  },
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
    deprecationSet$,
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
    prod: {},
  } as {
    dev: Map<PackageDiff>,
    nodeModulesOnly: Map<PackageDiff>,
    optional: Map<PackageDiff>,
    prod: Map<PackageDiff>,
  })

  const packageJson$ = most.fromPromise(
    most.merge(
      log$.packageJson.filter((log) => log.prefix === opts.prefix),
      log$.summary.filter((log) => log.prefix === opts.prefix).constant({}),
    )
    .take(2)
    .reduce(R.merge, {}),
  )

  return most.combine(
    (pkgsDiff, packageJsons) => {
      if (!packageJsons['initial'] || !packageJsons['updated']) return pkgsDiff

      const initialPackageJson = removeOptionalFromProdDeps(packageJsons['initial'])
      const updatedPackageJson = removeOptionalFromProdDeps(packageJsons['updated'])

      for (const depType of ['prod', 'optional', 'dev']) {
        const prop = propertyByDependencyType[depType]
        const initialDeps = R.keys(initialPackageJson[prop])
        const updatedDeps = R.keys(updatedPackageJson[prop])
        const removedDeps = R.difference(initialDeps, updatedDeps)

        for (const removedDep of removedDeps) {
          if (!pkgsDiff[depType][`-${removedDep}`]) {
            pkgsDiff[depType][`-${removedDep}`] = {
              added: false,
              name: removedDep,
              version: initialPackageJson[prop][removedDep],
            }
          }
        }

        const addedDeps = R.difference(updatedDeps, initialDeps)

        for (const addedDep of addedDeps) {
          if (!pkgsDiff[depType][`+${addedDep}`]) {
            pkgsDiff[depType][`+${addedDep}`] = {
              added: true,
              name: addedDep,
              version: updatedPackageJson[prop][addedDep],
            }
          }
        }
      }
      return pkgsDiff
    },
    pkgsDiff$,
    packageJson$,
  )
}

function removeOptionalFromProdDeps (pkg: PackageJson): PackageJson {
  if (!pkg.dependencies || !pkg.optionalDependencies) return pkg
  for (const depName of Object.keys(pkg.dependencies)) {
    if (pkg.optionalDependencies[depName]) {
      delete pkg.dependencies[depName]
    }
  }
  return pkg
}
