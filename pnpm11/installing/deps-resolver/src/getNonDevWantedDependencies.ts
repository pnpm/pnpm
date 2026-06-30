import type { Dependencies, DependenciesMeta, DependencyManifest } from '@pnpm/types'
import { pickBy } from 'ramda'

import { assertValidDependencyAliases } from './validateDependencyAlias.js'

export interface WantedDependency {
  alias: string
  bareSpecifier: string // package reference
  dev: boolean
  optional: boolean
  injected?: boolean
  saveCatalogName?: string
  /** Whether this dependency's spec should be (re)written to the manifest. */
  updateSpec?: boolean
}

type GetNonDevWantedDependenciesManifest = Pick<DependencyManifest, 'bundleDependencies' | 'bundledDependencies' | 'optionalDependencies' | 'dependencies' | 'dependenciesMeta'> & {
  name?: string
  version?: string
}

export function getNonDevWantedDependencies (pkg: GetNonDevWantedDependenciesManifest): WantedDependency[] {
  const pkgDescription = pkg.name != null
    ? `Package "${pkg.name}${pkg.version != null ? `@${pkg.version}` : ''}"`
    : 'Package'
  assertValidDependencyAliases(pkg.dependencies, pkgDescription)
  assertValidDependencyAliases(pkg.optionalDependencies, pkgDescription)
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
  return Object.entries(deps).map(([alias, bareSpecifier]) => ({
    alias,
    dev: !!opts.devDependencies[alias],
    injected: opts.dependenciesMeta[alias]?.injected,
    optional: !!opts.optionalDependencies[alias],
    bareSpecifier,
  }))
}

function getNotBundledDeps (bundledDeps: Set<string>, deps: Dependencies): Record<string, string> {
  return pickBy((_, depName) => !bundledDeps.has(depName), deps)
}
