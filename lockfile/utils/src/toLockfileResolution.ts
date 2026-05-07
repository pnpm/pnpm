import type { LockfileResolution } from '@pnpm/lockfile.types'
import type { Resolution, TarballResolution } from '@pnpm/resolving.resolver-base'
import getNpmTarballUrl from 'get-npm-tarball-url'

export function toLockfileResolution (
  pkg: {
    name: string
    version: string
  },
  resolution: Resolution,
  registry: string,
  lockfileIncludeTarballUrl?: boolean
): LockfileResolution {
  if (resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  // Tarball-typed resolutions are guaranteed to carry a tarball URL by the
  // resolver, but guard for unexpected inputs (e.g. resolutions deserialized
  // from external state) so we don't blow up on a missing field.
  const tarball = resolution['tarball'] as string | undefined
  if (tarball == null) {
    return { integrity: resolution['integrity'] }
  }
  // Honor the resolver-supplied flag, with a URL fallback for resolutions
  // that didn't go through the git resolver (e.g. config-dep migrations or
  // legacy lockfiles read by callers that don't enrich the field).
  const gitHosted = (resolution as TarballResolution).gitHosted === true ||
    isGitHostedTarballUrl(tarball)
  if (lockfileIncludeTarballUrl) {
    return preservingGitHosted({
      integrity: resolution['integrity'],
      tarball,
    }, gitHosted)
  }
  // Tarball URLs that cannot be reconstructed from the package name, version,
  // and registry must always stay in the lockfile, otherwise the package can
  // no longer be re-fetched. This covers local `file:` tarballs and tarballs
  // served by git providers (GitHub, GitLab, Bitbucket).
  if (tarball.startsWith('file:') || gitHosted) {
    return preservingGitHosted({
      integrity: resolution['integrity'],
      tarball,
    }, gitHosted)
  }
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in other weird cases, like https://github.com/pnpm/pnpm/issues/1072.
  // Even when the user explicitly sets `lockfileIncludeTarballUrl: false`, we
  // must preserve such URLs — otherwise the package cannot be re-fetched on a
  // frozen-lockfile install (e.g. GitHub Packages tarballs at
  // `https://npm.pkg.github.com/download/<scope>/<name>/<version>/<hash>`).
  // `lockfileIncludeTarballUrl` only controls whether URLs that *can* be
  // derived from name+version+registry are written.
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const actualTarball = tarball.replaceAll('%2f', '/')
  if (removeProtocol(expectedTarball) !== removeProtocol(actualTarball)) {
    return preservingGitHosted({
      integrity: resolution['integrity'],
      tarball,
    }, gitHosted)
  }
  return {
    integrity: resolution['integrity'],
  }
}

function preservingGitHosted<T extends { tarball?: string, integrity: string }> (
  resolution: T,
  gitHosted: boolean
): T & { gitHosted?: boolean } {
  return gitHosted ? { ...resolution, gitHosted: true } : resolution
}

// Inlined to avoid pulling @pnpm/fetching.pick-fetcher into the lockfile-utils
// dep graph. Used as a fallback when callers haven't pre-set the
// `gitHosted` field on TarballResolution.
function isGitHostedTarballUrl (url: string): boolean {
  return (
    url.startsWith('https://codeload.github.com/') ||
    url.startsWith('https://bitbucket.org/') ||
    url.startsWith('https://gitlab.com/')
  ) && url.includes('tar.gz')
}

function removeProtocol (url: string): string {
  return url.split('://')[1]
}
