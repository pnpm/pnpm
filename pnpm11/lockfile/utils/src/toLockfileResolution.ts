import type { LockfileResolution } from '@pnpm/lockfile.types'
import { isGitHostedTarballUrl, type Resolution, type TarballResolution } from '@pnpm/resolving.resolver-base'
import { isCanonicalRegistryTarballUrl } from '@pnpm/resolving.tarball-url'

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
  // A standard registry tarball whose URL can be rebuilt from the package name,
  // version, and registry is written as just `{ integrity }` — pnpm derives the
  // URL on demand. Every other tarball must keep its URL or it can no longer be
  // re-fetched on a frozen-lockfile install: `file:` tarballs, git-provider
  // tarballs (GitHub/GitLab/Bitbucket), and non-standard registry URLs such as
  // npm Enterprise (https://github.com/pnpm/pnpm/issues/867) or GitHub Packages
  // `/download/` URLs. `lockfileIncludeTarballUrl` forces the URL to be kept.
  if (
    !lockfileIncludeTarballUrl &&
    !gitHosted &&
    !tarball.startsWith('file:') &&
    isCanonicalRegistryTarballUrl(tarball, pkg, registry)
  ) {
    return { integrity: resolution['integrity'] }
  }
  // The kept-URL form carries the `gitHosted` marker and the subdirectory `path`
  // (`repo#commit&path:/sub/dir`, only ever set on git-hosted tarballs) so a
  // git-hosted monorepo tarball still unpacks the right subfolder.
  // See https://github.com/pnpm/pnpm/issues/12304.
  const { path } = resolution as TarballResolution
  return {
    integrity: resolution['integrity'],
    tarball,
    ...(gitHosted ? { gitHosted: true } : {}),
    ...(path == null ? {} : { path }),
  }
}
