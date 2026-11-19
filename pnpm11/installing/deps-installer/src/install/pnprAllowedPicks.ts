import { DEFAULT_REGISTRIES_BY_SCOPE } from '@pnpm/config.normalize-registries'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import type { Resolution, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'

import type { SingleProjectInstallOptions } from './mutationTypes.js'

export function violationsFromAllowedPicks (
  allowedPicks: string[],
  lockfile: LockfileObject,
  opts: Pick<SingleProjectInstallOptions, 'minimumReleaseAge' | 'minimumReleaseAgeStrict' | 'registriesByPrefix' | 'registriesByScope'>
): ResolutionPolicyViolation[] {
  assertAllowedPicksPolicy(allowedPicks, opts)
  const resolutions = resolutionsFromLockfile(lockfile, opts)
  return allowedPicks.flatMap((spec) => violationsFromAllowedPick(spec, resolutions))
}

function assertAllowedPicksPolicy (
  allowedPicks: string[],
  opts: Pick<SingleProjectInstallOptions, 'minimumReleaseAge' | 'minimumReleaseAgeStrict'>
): void {
  if (allowedPicks.length > 0 && (!opts.minimumReleaseAge || opts.minimumReleaseAgeStrict)) {
    throw new PnpmError(
      'PNPR_SERVER_INVALID_ALLOWED_PICKS',
      'The pnpr server returned allowed picks without an active loose minimumReleaseAge policy.'
    )
  }
}

function resolutionsFromLockfile (
  lockfile: LockfileObject,
  opts: Pick<SingleProjectInstallOptions, 'registriesByPrefix' | 'registriesByScope'>
): Map<string, Resolution> {
  const resolutions = new Map<string, Resolution>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    if (name != null && version != null && !isPatternName(name)) {
      resolutions.set(`${name}@${version}`, pkgSnapshotToResolution(depPath, snapshot, {
        ...opts,
        registriesByScope: opts.registriesByScope ?? DEFAULT_REGISTRIES_BY_SCOPE,
      }))
    }
  }
  return resolutions
}

function violationsFromAllowedPick (
  spec: string,
  resolutions: Map<string, Resolution>
): ResolutionPolicyViolation[] {
  const separator = spec.lastIndexOf('@')
  if (separator < 1) throw invalidAllowedPicks()
  const name = spec.slice(0, separator)
  if (isPatternName(name)) throw invalidAllowedPicks()
  const versions = spec.slice(separator + 1).split(' || ')
  if (versions.some((version) => version.length === 0)) throw invalidAllowedPicks()
  return versions.map((version) => {
    const resolution = resolutions.get(`${name}@${version}`)
    if (resolution == null) throw invalidAllowedPicks()
    return {
      name,
      version,
      resolution,
      code: 'MINIMUM_RELEASE_AGE_VIOLATION',
      reason: 'Version allowed under loose minimumReleaseAge policy',
    }
  })
}

function isPatternName (name: string): boolean {
  return name.startsWith('!') || name.includes('*') || name.includes('?')
}

function invalidAllowedPicks (): PnpmError {
  return new PnpmError(
    'PNPR_SERVER_INVALID_ALLOWED_PICKS',
    'The pnpr server returned an allowed pick that is not an exact version in the resolved lockfile.'
  )
}
