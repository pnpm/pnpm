import most = require('most')
import R = require('ramda')
import {
  DeprecationLog,
  Log,
} from 'supi'
import * as supi from 'supi'

export interface PackageDiff {
  added: boolean,
  from?: string,
  name: string,
  realName?: string,
  version?: string,
  deprecated?: boolean,
  latest?: string,
  linked?: true,
}

export interface Map<T> {
  [index: string]: T,
}

export const propertyByDependencyType = {
  dev: 'devDependencies',
  optional: 'optionalDependencies',
  prod: 'dependencies',
}

export default function (
  log$: {
    deprecation: most.Stream<supi.DeprecationLog>,
    summary: most.Stream<supi.SummaryLog>,
    root: most.Stream<supi.RootLog>,
    packageJson: most.Stream<supi.PackageJsonLog>,
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
      pkgsDiff[rootLog['added'].dependencyType][`+${rootLog['added'].name}`] = {
        added: true,
        deprecated: deprecationSet.has(rootLog['added'].id),
        latest: rootLog['added'].latest,
        name: rootLog['added'].name,
        realName: rootLog['added'].realName,
        version: rootLog['added'].version,
      }
      return pkgsDiff
    }
    if (rootLog['removed']) {
      pkgsDiff[rootLog['removed'].dependencyType][`-${rootLog['removed'].name}`] = {
        added: false,
        name: rootLog['removed'].name,
        version: rootLog['removed'].version,
      }
      return pkgsDiff
    }
    if (rootLog['linked']) {
      pkgsDiff[rootLog['linked'].dependencyType][`>${rootLog['linked'].name}`] = {
        added: false,
        from: rootLog['linked'].from,
        linked: true,
        name: rootLog['linked'].name,
      }
      return pkgsDiff
    }
    return pkgsDiff
  }, {
    dev: {},
    optional: {},
    prod: {},
  } as {
    dev: Map<PackageDiff>,
    prod: Map<PackageDiff>,
    optional: Map<PackageDiff>,
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
      const initialPackageJson = packageJsons['initial']
      const updatedPackageJson = packageJsons['updated']

      if (!initialPackageJson || !updatedPackageJson) return pkgsDiff

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
