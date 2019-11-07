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
  opts?: {
    updateWorkspaceDependencies?: boolean,
  }
): WantedDependency[] {
  const depsToInstall = getAllDependenciesFromPackage(pkg)
  return getWantedDependenciesFromGivenSet(depsToInstall, {
    devDependencies: pkg.devDependencies || {},
    optionalDependencies: pkg.optionalDependencies || {},
    updatePref: opts?.updateWorkspaceDependencies === true
      ? updateWorkspacePref
      : (pref) => pref,
  })
}

function updateWorkspacePref (pref: string) {
  return pref.startsWith('workspace:') ? 'workspace:*' : pref
}

function getWantedDependenciesFromGivenSet (
  deps: Dependencies,
  opts: {
    devDependencies: Dependencies,
    optionalDependencies: Dependencies,
    updatePref: (pref: string) => string,
  },
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map((alias) => {
    const pref = opts.updatePref(deps[alias])
    return {
      alias,
      dev: !!opts.devDependencies[alias],
      optional: !!opts.optionalDependencies[alias],
      pref,
      raw: `${alias}@${pref}`,
    }
  })
}
