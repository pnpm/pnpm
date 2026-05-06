import type { LockfileResolution } from '@pnpm/lockfile.types'
import type { Resolution } from '@pnpm/resolving.resolver-base'
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
  // Defensive promotion for resolutions that didn't go through the git
  // resolver (config-dep migrations, legacy paths). The git resolver and
  // the lockfile loader normally set this already.
  if (
    resolution.type === undefined &&
    resolution['integrity'] != null &&
    resolution['tarball'] != null &&
    isGitHostedTarballUrl(resolution['tarball'] as string)
  ) {
    resolution = { ...resolution, type: 'git-tarball' } as Resolution
  }
  if (resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  // Past this point: npm-registry tarball with integrity.
  const tarball = resolution['tarball'] as string | undefined
  if (lockfileIncludeTarballUrl) {
    return { integrity: resolution['integrity'], tarball }
  }
  // Local `file:` tarballs can't be reconstructed from the package
  // name/version + registry, so the URL must stay.
  if (tarball != null && tarball.startsWith('file:')) {
    return { integrity: resolution['integrity'], tarball }
  }
  if (lockfileIncludeTarballUrl === false) {
    return { integrity: resolution['integrity'] }
  }
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in other weird cases, like https://github.com/pnpm/pnpm/issues/1072
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const actualTarball = tarball!.replaceAll('%2f', '/')
  if (removeProtocol(expectedTarball) !== removeProtocol(actualTarball)) {
    return { integrity: resolution['integrity'], tarball }
  }
  return { integrity: resolution['integrity'] }
}

// Inlined to avoid pulling @pnpm/fetching.pick-fetcher into the lockfile-utils
// dep graph. Used as a fallback for resolutions handed to this function
// without the `git-tarball` type discriminator.
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
