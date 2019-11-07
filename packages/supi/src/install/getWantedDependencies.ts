import { Dependencies, ImporterManifest } from '@pnpm/types'
import { getAllDependenciesFromPackage } from '@pnpm/utils'

export interface WantedDependency {
  alias: string,
  pref: string, // package reference
  dev: boolean,
  optional: boolean,
  raw: string, // might be not needed
}

export default function getWantedDependencies (
  pkg: Pick<ImporterManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
): WantedDependency[] {
  const depsToInstall = getAllDependenciesFromPackage(pkg)
  return getWantedDependenciesFromGivenSet(depsToInstall, {
    devDependencies: pkg.devDependencies || {},
    optionalDependencies: pkg.optionalDependencies || {},
  })
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
