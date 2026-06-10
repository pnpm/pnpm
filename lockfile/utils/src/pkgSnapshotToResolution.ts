import url from 'url'
import { PnpmError } from '@pnpm/error'
import { type PackageSnapshot, type TarballResolution } from '@pnpm/lockfile.types'
import { type Resolution } from '@pnpm/resolver-base'
import { type Registries } from '@pnpm/types'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { assertRegistryShapedResolution } from './assertRegistryShapedResolution.js'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  assertRegistryShapedResolution(depPath, pkgSnapshot)
  const resolution = pkgSnapshot.resolution as TarballResolution
  // Tarball-shaped resolutions (no `type` field) must carry `integrity`,
  // except where the URL itself anchors the bytes:
  //   - `file:` tarballs (local file on the user's machine; integrity
  //     adds nothing the user doesn't already control).
  //   - Git-hosted tarballs (URL contains the commit SHA; git's content-
  //     addressed model binds the bytes to the commit). The `gitHosted`
  //     flag may be absent on legacy lockfiles, so fall back to a URL
  //     match against the known git-host download endpoints.
  // For any other tarball entry a missing integrity is what a tampered
  // lockfile looks like: the worker would mint a fresh integrity from
  // whatever bytes the URL returned, so we fail closed here.
  if (
    resolution.type == null &&
    resolution.integrity == null &&
    !resolution.tarball?.startsWith('file:') &&
    !(resolution.gitHosted === true || (resolution.tarball != null && isGitHostedTarballUrl(resolution.tarball)))
  ) {
    throw new PnpmError('MISSING_TARBALL_INTEGRITY',
      `Cannot install package "${depPath}": its lockfile entry has no "integrity" field, so pnpm cannot verify the downloaded tarball.`,
      { hint: 'The lockfile may be corrupted or have been tampered with. Restore it from a trusted source, or delete it and re-run installation without --frozen-lockfile to regenerate.' }
    )
  }
  if (
    Boolean(resolution.type) ||
    resolution.tarball?.startsWith('file:') ||
    resolution.gitHosted === true
  ) {
    return pkgSnapshot.resolution as Resolution
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
  if (!(pkgSnapshot.resolution as TarballResolution).tarball) {
    tarball = getTarball(registry)
  } else {
    tarball = new url.URL((pkgSnapshot.resolution as TarballResolution).tarball,
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

// Fallback for legacy lockfile entries whose tarball resolution lacks the
// `gitHosted` flag. Matches the known git-host download endpoints so a URL
// rewritten by pnpm's git resolver is still recognized as content-addressed
// and exempt from the integrity requirement.
function isGitHostedTarballUrl (url: string): boolean {
  return (
    url.startsWith('https://codeload.github.com/') ||
    url.startsWith('https://bitbucket.org/') ||
    url.startsWith('https://gitlab.com/')
  )
}
