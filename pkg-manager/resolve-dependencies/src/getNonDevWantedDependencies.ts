import { type Dependencies, type DependencyManifest, type DependenciesMeta } from '@pnpm/types'
import pickBy from 'ramda/src/pickBy'

export interface WantedDependency {
  alias: string
  pref: string // package reference
  dev: boolean
  optional: boolean
  injected?: boolean
}

type GetNonDevWantedDependenciesManifest = Pick<DependencyManifest, 'bundleDependencies' | 'bundledDependencies' | 'optionalDependencies' | 'dependencies' | 'dependenciesMeta'>

export function getNonDevWantedDependencies (pkg: GetNonDevWantedDependenciesManifest, opts: { injectWorkspacePackages?: boolean }): WantedDependency[] {
  let bd = pkg.bundledDependencies ?? pkg.bundleDependencies
  if (bd === true) {
    bd = pkg.dependencies != null ? Object.keys(pkg.dependencies) : []
  }
  const bundledDeps = new Set(Array.isArray(bd) ? bd : [])
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  return getWantedDependenciesFromGivenSet(
    filterDeps({ ...pkg.optionalDependencies, ...pkg.dependencies }),
    {
      dependenciesMeta: pkg.dependenciesMeta ?? {},
      devDependencies: {},
      optionalDependencies: pkg.optionalDependencies ?? {},
      injectWorkspacePackages: opts.injectWorkspacePackages,
    }
  )
}

function getWantedDependenciesFromGivenSet (
  deps: Dependencies,
  opts: {
    devDependencies: Dependencies
    optionalDependencies: Dependencies
    dependenciesMeta: DependenciesMeta
    injectWorkspacePackages?: boolean
  }
): WantedDependency[] {
  if (!deps) return []
  return Object.entries(deps).map(([alias, pref]) => ({
    alias,
    dev: !!opts.devDependencies[alias],
    injected: opts.injectWorkspacePackages === true || opts.dependenciesMeta[alias]?.injected,
    optional: !!opts.optionalDependencies[alias],
    pref,
  }))
}

function getNotBundledDeps (bundledDeps: Set<string>, deps: Dependencies): Record<string, string> {
  return pickBy((_, depName) => !bundledDeps.has(depName), deps)
}
