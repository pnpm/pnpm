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
  const tarball = resolution['tarball'] as string | undefined
  // Honor the resolver-supplied flag, with a URL fallback for resolutions
  // that didn't go through the git resolver (e.g. config-dep migrations or
  // legacy lockfiles read by callers that don't enrich the field).
  const gitHosted = (resolution as TarballResolution).gitHosted === true ||
    (tarball != null && isGitHostedTarballUrl(tarball))
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
  if (tarball != null && (tarball.startsWith('file:') || gitHosted)) {
    return preservingGitHosted({
      integrity: resolution['integrity'],
      tarball,
    }, gitHosted)
  }
  if (lockfileIncludeTarballUrl === false) {
    return {
      integrity: resolution['integrity'],
    }
  }
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in other weird cases, like https://github.com/pnpm/pnpm/issues/1072
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const actualTarball = tarball!.replaceAll('%2f', '/')
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
