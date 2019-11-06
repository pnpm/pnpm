import parsePref, { RegistryPackageSpec } from '@pnpm/npm-resolver/lib/parsePref'
import { LocalPackages } from '@pnpm/resolver-base'
import { PackageManifest } from '@pnpm/types'
import { WantedDependency } from '@pnpm/utils'
import semver = require('semver')

export default function wantedDepIsLocallyAvailable (
  localPackages: LocalPackages,
  wantedDependency: Omit<WantedDependency, 'raw'>,
  opts: {
    defaultTag: string,
    registry: string,
  },
) {
  const spec = parsePref(wantedDependency.pref, wantedDependency.alias, opts.defaultTag || 'latest', opts.registry)
  if (!spec || !localPackages[spec.name]) return false
  return pickMatchingLocalVersionOrNull(localPackages[spec.name], spec) !== null
}

// TODO: move this function to separate package or import from @pnpm/npm-resolver
function pickMatchingLocalVersionOrNull (
  versions: {
    [version: string]: {
      dir: string;
      manifest: PackageManifest;
    },
  },
  spec: RegistryPackageSpec,
) {
  const localVersions = Object.keys(versions)
  switch (spec.type) {
    case 'tag':
      return semver.maxSatisfying(localVersions, '*')
    case 'version':
      return versions[spec.fetchSpec] ? spec.fetchSpec : null
    case 'range':
      return semver.maxSatisfying(localVersions, spec.fetchSpec, true)
    default:
      return null
  }
}
