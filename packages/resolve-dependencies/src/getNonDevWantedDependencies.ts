import { Dependencies, DependencyManifest, DependenciesMeta } from '@pnpm/types'

export interface WantedDependency {
  alias: string
  pref: string // package reference
  dev: boolean
  optional: boolean
  injected?: boolean
}

export default function getNonDevWantedDependencies (pkg: DependencyManifest) {
  const bd = pkg.bundleDependencies ?? pkg.bundleDependencies
  const bundledDeps = new Set(Array.isArray(bd) ? bd : [])
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  return getWantedDependenciesFromGivenSet(
    filterDeps({ ...pkg.optionalDependencies, ...pkg.dependencies }),
    {
      dependenciesMeta: pkg.dependenciesMeta ?? {},
      devDependencies: {},
      optionalDependencies: pkg.optionalDependencies ?? {},
    }
  )
}

function getWantedDependenciesFromGivenSet (
  deps: Dependencies,
  opts: {
    devDependencies: Dependencies
    optionalDependencies: Dependencies
    dependenciesMeta: DependenciesMeta
  }
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map((alias) => ({
    alias,
    dev: !!opts.devDependencies[alias],
    injected: opts.dependenciesMeta[alias]?.injected,
    optional: !!opts.optionalDependencies[alias],
    pref: deps[alias],
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
