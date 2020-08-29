import { filterDependenciesByType } from '@pnpm/manifest-utils'
import {
  Dependencies,
  IncludedDependencies,
  ProjectManifest,
} from '@pnpm/types'
import guessPinnedVersionFromExistingSpec from '../guessPinnedVersionFromExistingSpec'

export type PinnedVersion = 'major' | 'minor' | 'patch' | 'none'

export interface WantedDependency {
  alias: string
  pref: string // package reference
  dev: boolean
  optional: boolean
  raw: string
  pinnedVersion?: PinnedVersion
}

export default function getWantedDependencies (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
  opts?: {
    includeDirect?: IncludedDependencies
    updateWorkspaceDependencies?: boolean
  }
): WantedDependency[] {
  const depsToInstall = filterDependenciesByType(pkg,
    opts?.includeDirect ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    })
  return getWantedDependenciesFromGivenSet(depsToInstall, {
    dependencies: pkg.dependencies ?? {},
    devDependencies: pkg.devDependencies ?? {},
    optionalDependencies: pkg.optionalDependencies ?? {},
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
    dependencies: Dependencies
    devDependencies: Dependencies
    optionalDependencies: Dependencies
    updatePref: (pref: string) => string
  }
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map((alias) => {
    const pref = opts.updatePref(deps[alias])
    let depType
    if (opts.optionalDependencies[alias] != null) depType = 'optional'
    else if (opts.dependencies[alias] != null) depType = 'prod'
    else if (opts.devDependencies[alias] != null) depType = 'dev'
    return {
      alias,
      dev: depType === 'dev',
      optional: depType === 'optional',
      pinnedVersion: guessPinnedVersionFromExistingSpec(deps[alias]),
      pref,
      raw: `${alias}@${pref}`,
    }
  })
}
