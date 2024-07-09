import { parsePref, type RegistryPackageSpec } from '@pnpm/npm-resolver'
import { type WorkspacePackagesByVersion, type WorkspacePackages } from '@pnpm/resolver-base'
import semver from 'semver'
import { type WantedDependency } from './getNonDevWantedDependencies'

export function wantedDepIsLocallyAvailable (
  workspacePackages: WorkspacePackages,
  wantedDependency: WantedDependency,
  opts: {
    defaultTag: string
    registry: string
  }
): boolean {
  const spec = parsePref(wantedDependency.pref, wantedDependency.alias, opts.defaultTag || 'latest', opts.registry)
  if ((spec == null) || !workspacePackages.has(spec.name)) return false
  return pickMatchingLocalVersionOrNull(workspacePackages.get(spec.name)!, spec) !== null
}

// TODO: move this function to separate package or import from @pnpm/npm-resolver
function pickMatchingLocalVersionOrNull (
  versions: WorkspacePackagesByVersion,
  spec: RegistryPackageSpec
): string | null {
  const localVersions = Object.keys(versions)
  switch (spec.type) {
  case 'tag':
    return semver.maxSatisfying(localVersions, '*')
  case 'version':
    return versions.has(spec.fetchSpec) ? spec.fetchSpec : null
  case 'range':
    return semver.maxSatisfying(localVersions, spec.fetchSpec, true)
  default:
    return null
  }
}
