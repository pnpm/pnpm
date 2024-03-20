import type {
  Dependencies,
  PackageManifest,
  ProjectManifest,
  WantedDependency,
  DependenciesMeta,
} from '@pnpm/types'
import pickBy from 'ramda/src/pickBy'

export function getNonDevWantedDependencies(
  pkg: PackageManifest | ProjectManifest | undefined
) {
  let bd = pkg?.bundledDependencies ?? pkg?.bundleDependencies

  if (bd === true) {
    bd = pkg?.dependencies != null ? Object.keys(pkg.dependencies) : []
  }

  const bundledDeps = new Set(Array.isArray(bd) ? bd : [])

  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)

  return getWantedDependenciesFromGivenSet(
    filterDeps({ ...pkg?.optionalDependencies, ...pkg?.dependencies }),
    {
      dependenciesMeta: pkg?.dependenciesMeta ?? {},
      devDependencies: {},
      optionalDependencies: pkg?.optionalDependencies ?? {},
    }
  )
}

function getWantedDependenciesFromGivenSet(
  deps: Dependencies,
  opts: {
    devDependencies: Dependencies
    optionalDependencies: Dependencies
    dependenciesMeta: DependenciesMeta
  }
): WantedDependency[] {
  if (!deps) {
    return []
  }

  return Object.entries(deps).map(([alias, pref]: [string, string]): {
    alias: string;
    dev: boolean;
    injected: boolean | undefined;
    optional: boolean;
    pref: string;
  } => {
    return {
      alias,
      dev: !!opts.devDependencies[alias],
      injected: opts.dependenciesMeta[alias]?.injected,
      optional: !!opts.optionalDependencies[alias],
      pref,
    };
  })
}

function getNotBundledDeps(
  bundledDeps: Set<string>,
  deps: Dependencies
): Record<string, string> {
  return pickBy((_, depName) => !bundledDeps.has(depName), deps)
}
