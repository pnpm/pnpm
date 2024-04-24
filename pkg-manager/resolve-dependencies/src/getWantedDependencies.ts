import { filterDependenciesByType } from '@pnpm/manifest-utils'
import {
  type Dependencies,
  type DependenciesMeta,
  type IncludedDependencies,
  type ProjectManifest,
} from '@pnpm/types'
import { whichVersionIsPinned } from '@pnpm/which-version-is-pinned'
import { parseWorkspacePref } from '@pnpm/workspace-pref'

export type PinnedVersion = 'major' | 'minor' | 'patch' | 'none'

export interface WantedDependency {
  alias: string
  pref: string // package reference
  dev: boolean
  optional: boolean
  raw: string
  pinnedVersion?: PinnedVersion
  nodeExecPath?: string
  updateSpec?: boolean
}

export function getWantedDependencies (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies' | 'dependenciesMeta' | 'peerDependencies'>,
  opts?: {
    autoInstallPeers?: boolean
    includeDirect?: IncludedDependencies
    nodeExecPath?: string
    updateWorkspaceDependencies?: boolean
  }
): WantedDependency[] {
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
    updatePref: opts?.updateWorkspaceDependencies === true
      ? updateWorkspacePref
      : (pref) => pref,
  })
}

function updateWorkspacePref (pref: string): string {
  const parseResult = parseWorkspacePref(pref)
  if (!parseResult) return pref
  const { alias } = parseResult
  return alias ? `workspace:${alias}@*` : 'workspace:*'
}

function getWantedDependenciesFromGivenSet (
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
  if (!deps) return []
  return Object.entries(deps).map(([alias, pref]) => {
    const updatedPref = opts.updatePref(pref)
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
      nodeExecPath: opts.nodeExecPath ?? opts.dependenciesMeta[alias]?.node,
      pinnedVersion: whichVersionIsPinned(pref),
      pref: updatedPref,
      raw: `${alias}@${pref}`,
    }
  })
}
