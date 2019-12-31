import { Dependencies, ProjectManifest } from '@pnpm/types'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
import guessPinnedVersionFromExistingSpec from '../guessPinnedVersionFromExistingSpec'

export type PinnedVersion = 'major' | 'minor' | 'patch' | 'none'

export interface WantedDependency {
  alias: string,
  pref: string, // package reference
  dev: boolean,
  optional: boolean,
  raw: string,
  pinnedVersion?: PinnedVersion,
}

export default function getWantedDependencies (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
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
      pinnedVersion: guessPinnedVersionFromExistingSpec(deps[alias]),
      pref,
      raw: `${alias}@${pref}`,
    }
  })
}
