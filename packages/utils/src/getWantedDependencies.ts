import { Dependencies, DependencyPackageJson, PackageJson } from '@pnpm/types'
import depsFromPackage from './getAllDependenciesFromPackage'

export interface WantedDependency {
  alias?: string,
  pref: string, // package reference
  dev: boolean,
  optional: boolean,
  raw: string, // might be not needed
}

export function getWantedDependencies (pkg: PackageJson): WantedDependency[] {
  const depsToInstall = depsFromPackage(pkg)
  return getWantedDependenciesFromGivenSet(depsToInstall, {
    devDependencies: pkg.devDependencies || {},
    optionalDependencies: pkg.optionalDependencies || {},
  })
}

export function getNonDevWantedDependencies (pkg: DependencyPackageJson) {
  const bundledDeps = new Set(pkg.bundleDependencies || pkg.bundledDependencies || [])
  bundledDeps.add(pkg.name)
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  return getWantedDependenciesFromGivenSet(
    filterDeps({ ...pkg.optionalDependencies, ...pkg.dependencies }),
    {
      devDependencies: {},
      optionalDependencies: pkg.optionalDependencies || {},
    },
  )
}

function getWantedDependenciesFromGivenSet (
  deps: Dependencies,
  opts: {
    devDependencies: Dependencies,
    optionalDependencies: Dependencies,
  },
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map((alias) => ({
    alias,
    dev: !!opts.devDependencies[alias],
    optional: !!opts.optionalDependencies[alias],
    pref: deps[alias],
    raw: `${alias}@${deps[alias]}`,
  }))
}

function getNotBundledDeps (bundledDeps: Set<string>, deps: Dependencies) {
  return Object.keys(deps)
    .filter((depName) => !bundledDeps.has(depName))
    .reduce((notBundledDeps, depName) => {
      notBundledDeps[depName] = deps[depName]
      return notBundledDeps
    }, {})
}
