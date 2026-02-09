import { filterDependenciesByType } from '@pnpm/manifest-utils'
import {
  type Dependencies,
  type DependenciesMeta,
  type IncludedDependencies,
  type ProjectManifest,
} from '@pnpm/types'

export interface WantedDependency {
  alias: string
  bareSpecifier: string // package reference
  dev: boolean
  optional: boolean
  saveCatalogName?: string
  updateSpec?: boolean
  prevSpecifier?: string
}

export function getWantedDependencies (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies' | 'dependenciesMeta' | 'peerDependencies'>,
  opts?: {
    autoInstallPeers?: boolean
    includeDirect?: IncludedDependencies
  }
): WantedDependency[] {
  const depsToInstall = filterDependenciesByType(pkg,
    opts?.includeDirect ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    { autoInstallPeers: opts?.autoInstallPeers })
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
): WantedDependency[] {
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
