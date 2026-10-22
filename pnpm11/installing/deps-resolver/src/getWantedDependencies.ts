import { filterDependenciesByType } from '@pnpm/pkg-manifest.utils'
import type {
  Dependencies,
  DependenciesMeta,
  IncludedDependencies,
  ProjectManifest,
} from '@pnpm/types'

import { assertValidDependencyAliases } from './validateDependencyAlias.js'

export interface WantedDependency {
  alias?: string
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
  prevSpecifier?: string
}

export type ManifestWantedDependency = WantedDependency & { alias: string }

export function hasAlias<Dependency extends WantedDependency> (wantedDependency: Dependency): wantedDependency is Dependency & { alias: string } {
  return wantedDependency.alias != null
}

export function getWantedDependencies (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies' | 'dependenciesMeta' | 'peerDependencies'>,
  opts?: {
    autoInstallPeers?: boolean
    includeDirect?: IncludedDependencies
  }
): ManifestWantedDependency[] {
  assertValidDependencyAliases(pkg.dependencies, 'The current package')
  assertValidDependencyAliases(pkg.devDependencies, 'The current package')
  assertValidDependencyAliases(pkg.optionalDependencies, 'The current package')
  assertValidDependencyAliases(pkg.peerDependencies, 'The current package')
  let depsToInstall = filterDependenciesByType(pkg,
    opts?.includeDirect ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    })
  if (opts?.autoInstallPeers) {
    depsToInstall = {
      ...pkg.peerDependencies,
      ...depsToInstall,
    }
  }
  return getWantedDependenciesFromGivenSet(depsToInstall, {
    dependencies: pkg.dependencies ?? {},
    devDependencies: pkg.devDependencies ?? {},
    optionalDependencies: pkg.optionalDependencies ?? {},
    dependenciesMeta: pkg.dependenciesMeta ?? {},
    peerDependencies: pkg.peerDependencies ?? {},
  })
}

function getWantedDependenciesFromGivenSet (
  deps: Dependencies,
  opts: {
    dependencies: Dependencies
    devDependencies: Dependencies
    optionalDependencies: Dependencies
    peerDependencies: Dependencies
    dependenciesMeta: DependenciesMeta
  }
): ManifestWantedDependency[] {
  if (!deps) return []
  return Object.entries(deps).map(([alias, bareSpecifier]) => {
    let depType
    if (opts.optionalDependencies[alias] != null) depType = 'optional'
    else if (opts.dependencies[alias] != null) depType = 'prod'
    else if (opts.devDependencies[alias] != null) depType = 'dev'
    else if (opts.peerDependencies[alias] != null) depType = 'prod'
    return {
      alias,
      dev: depType === 'dev',
      injected: opts.dependenciesMeta[alias]?.injected,
      optional: depType === 'optional',
      bareSpecifier,
      prevSpecifier: bareSpecifier,
    }
  })
}
