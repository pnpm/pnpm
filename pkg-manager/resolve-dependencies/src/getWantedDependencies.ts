import type {
  Dependencies,
  ProjectManifest,
  DependenciesMeta,
  WantedDependency,
  IncludedDependencies,
} from '@pnpm/types'
import { filterDependenciesByType } from '@pnpm/manifest-utils'
import { whichVersionIsPinned } from '@pnpm/which-version-is-pinned'

export function getWantedDependencies(
  pkg: Pick<
    ProjectManifest,
    | 'devDependencies'
    | 'dependencies'
    | 'optionalDependencies'
    | 'dependenciesMeta'
    | 'peerDependencies'
  > | undefined,
  opts?: {
    autoInstallPeers?: boolean | undefined
    includeDirect?: IncludedDependencies | undefined
    nodeExecPath?: string | undefined
    updateWorkspaceDependencies?: boolean | undefined
  } | undefined
): WantedDependency[] {
  let depsToInstall = filterDependenciesByType(
    pkg,
    opts?.includeDirect ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    }
  )

  if (opts?.autoInstallPeers) {
    depsToInstall = {
      ...pkg?.peerDependencies,
      ...depsToInstall,
    }
  }

  return getWantedDependenciesFromGivenSet(depsToInstall, {
    dependencies: pkg?.dependencies ?? {},
    devDependencies: pkg?.devDependencies ?? {},
    optionalDependencies: pkg?.optionalDependencies ?? {},
    dependenciesMeta: pkg?.dependenciesMeta ?? {},
    peerDependencies: pkg?.peerDependencies ?? {},
    updatePref:
      opts?.updateWorkspaceDependencies === true
        ? updateWorkspacePref
        : (pref) => pref,
  })
}

function updateWorkspacePref(pref: string): string {
  return pref.startsWith('workspace:') ? 'workspace:*' : pref
}

function getWantedDependenciesFromGivenSet(
  deps: Dependencies,
  opts: {
    dependencies: Dependencies
    devDependencies: Dependencies
    optionalDependencies: Dependencies
    peerDependencies: Dependencies
    dependenciesMeta: DependenciesMeta
    nodeExecPath?: string
    updatePref: (pref: string) => string
  }
): WantedDependency[] {
  if (!deps) {
    return []
  }

  return Object.entries(deps).map(([alias, pref]) => {
    const updatedPref = opts.updatePref(pref)

    let depType

    if (typeof opts.optionalDependencies[alias] !== 'undefined') {
      depType = 'optional'
    } else if (typeof opts.dependencies[alias] !== 'undefined') {
      depType = 'prod'
    } else if (typeof opts.devDependencies[alias] !== 'undefined') {
      depType = 'dev'
    } else if (typeof opts.peerDependencies[alias] !== 'undefined') {
      depType = 'prod'
    }

    return {
      alias,
      dev: depType === 'dev',
      injected: opts.dependenciesMeta[alias]?.injected,
      optional: depType === 'optional',
      nodeExecPath: opts.nodeExecPath ?? opts.dependenciesMeta[alias]?.node,
      pinnedVersion: whichVersionIsPinned(pref),
      pref: updatedPref,
      raw: `${alias}@${pref}`,
    }
  })
}
