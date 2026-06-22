import url from 'node:url'

import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import { classifyResolution, type Resolution } from '@pnpm/resolving.resolver-base'
import { getNpmTarballUrl } from '@pnpm/resolving.tarball-url'
import type { Registries } from '@pnpm/types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

// A registry tarball entry that lacks an `integrity` is rejected by the npm
// resolver's lockfile verifier (`MISSING_TARBALL_INTEGRITY`), so the read-side
// policy enforcement lives there rather than in this pure snapshot→resolution
// conversion. `assertFetchableResolution` (below) is the fail-closed companion that
// must gate the *fetch* itself.
export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  const resolution = pkgSnapshot.resolution as TarballResolution
  if (resolution.tarball != null && typeof resolution.tarball !== 'string') {
    // Lockfiles are untrusted; a non-string `tarball` (e.g. a YAML array) would otherwise be
    // string-coerced into an attacker-controlled URL by `new url.URL(...)`, and crash the
    // string checks below. Fail closed.
    throw new PnpmError('INVALID_TARBALL_RESOLUTION',
      `Cannot install package "${depPath}": its lockfile entry has a non-string "tarball" field.`)
  }
  if (
    Boolean(resolution.type) ||
    resolution.tarball?.startsWith('file:') ||
    resolution.gitHosted === true
  ) {
    return pkgSnapshot.resolution as Resolution
  }
  // Legacy `file:` snapshots may carry the tarball in the depPath only.
  const nonSemverVersion = dp.parse(depPath).nonSemverVersion
  if (nonSemverVersion?.startsWith('file:')) {
    return {
      ...pkgSnapshot.resolution,
      tarball: nonSemverVersion,
    } as Resolution
  }
  const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  let registry: string = ''
  if (name != null) {
    if (name[0] === '@') {
      registry = registries[name.split('/')[0]]
    }
  }
  if (!registry) {
    registry = registries.default
  }
  let tarball!: string
  if (!resolution.tarball) {
    tarball = getTarball(registry)
  } else {
    tarball = new url.URL(resolution.tarball,
      registry.endsWith('/') ? registry : `${registry}/`
    ).toString()
  }
  return {
    ...pkgSnapshot.resolution,
    tarball,
  } as Resolution

  function getTarball (registry: string) {
    if (!name || !version) {
      throw new Error(`Couldn't get tarball URL from dependency path ${depPath}`)
    }
    return getNpmTarballUrl(name, version, { registry })
  }
}

/**
 * Fail closed before a lockfile-derived resolution is handed to the store controller to
 * fetch. A registry/`http(s)` tarball (`remoteTarball`) must carry a non-empty string
 * `integrity`, otherwise its downloaded bytes can't be checked against an expected hash.
 *
 * The npm resolver's lockfile verifier enforces the same rule, but it runs in
 * parallel with fetching and some headless paths do not run it, so fetch sites
 * keep the same cheap network-free guard.
 *
 * `file:`, git-hosted, git, directory, binary and custom resolutions are anchored another
 * way (local bytes, a commit SHA) and are exempt.
 */
export function assertFetchableResolution (depPath: string, resolution: Resolution): void {
  if (classifyResolution(resolution) !== 'remoteTarball') return
  const integrity = (resolution as { integrity?: unknown }).integrity
  if (typeof integrity !== 'string' || integrity.length === 0) {
    throw new PnpmError('MISSING_TARBALL_INTEGRITY',
      `Cannot fetch package "${depPath}" from the lockfile: it has no "integrity" field, so the downloaded tarball cannot be verified. Run a fresh install to repair the lockfile.`)
  }
}
