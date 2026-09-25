import type { Dependencies, DependenciesMeta, DependencyManifest } from '@pnpm/types'
import { pickBy } from 'ramda'

import { assertValidDependencyAliases } from './validateDependencyAlias.js'

export interface WantedDependency {
  alias: string
  bareSpecifier: string // package reference
  dev: boolean
  /** Whether this run is the one that adds the dependency to the project's manifest. */
  isNew?: boolean
  optional: boolean
  injected?: boolean
  saveCatalogName?: string
  /**
   * `false` keeps the spec out of the manifest entirely: something other than the project
   * declares it — a `packageExtensions` entry, a `readPackage` hook, or an override — and that
   * declaration is not this run's to move. `true` marks a spec the run was asked for by name,
   * which outranks such a declaration.
   */
  saveSpec?: boolean
  /**
   * `false` keeps `--latest` from resolving past the specifier this dependency was given: the
   * specifier is not the run's to move, so a resolution that ignores it would leave the lockfile
   * claiming a version its own specifier rejects. A compatible update still applies, since that
   * one honors the specifier.
   */
  updateToLatestAllowed?: boolean
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
