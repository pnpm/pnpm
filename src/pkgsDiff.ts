import R = require('ramda')
import {
  DeprecationLog,
  Log,
} from 'supi'
import xs, {Stream} from 'xstream'

export interface PackageDiff {
  name: string,
  version?: string,
  added: boolean,
  deprecated?: boolean,
}

interface Map<T> {
  [index: string]: T,
}

export const propertyByDependencyType = {
  dev: 'devDependencies',
  optional: 'optionalDependencies',
  prod: 'dependencies',
}

export default (log$: xs<Log>, deprecationLog$: xs<DeprecationLog>) => {
  const rootLog$ = log$.filter((log) => log.name === 'pnpm:root')

  const deprecationSet$ = deprecationLog$
    .fold((acc, log) => {
      acc.add(log.pkgId)
      return acc
    }, new Set())

  const pkgsDiff$ = xs.combine(
    rootLog$,
    deprecationSet$,
  )
  .fold((pkgsDiff, args) => {
    const rootLog = args[0]
    const deprecationSet = args[1] as Set<string>
    if (rootLog['added']) {
      pkgsDiff[rootLog['added'].dependencyType][`+${rootLog['added'].name}`] = {
        added: true,
        deprecated: deprecationSet.has(rootLog['added'].id),
        name: rootLog['added'].name,
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

  const packageJson$ = log$
    .filter((log) => log.name === 'pnpm:package-json')
    .take(2)
    .fold(R.merge, {})
    .last()

  return xs.combine(pkgsDiff$, packageJson$)
    .map(R.apply((pkgsDiff, packageJsons) => {
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
    }))
}
