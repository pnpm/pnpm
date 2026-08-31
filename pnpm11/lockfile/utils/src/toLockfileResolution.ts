import { PnpmError } from '@pnpm/error'
import type { LockfileResolution } from '@pnpm/lockfile.types'
import { type GitResolution, isGitHostedTarballUrl, type Resolution, type TarballResolution } from '@pnpm/resolving.resolver-base'
import {
  isCanonicalRegistryTarballUrl,
  isIntegrityAddressedRegistryTarballUrl,
  isValidTarballRevision,
} from '@pnpm/resolving.tarball-url'
import type { RegistryServerType } from '@pnpm/types'

export interface ToLockfileResolutionOptions {
  registry: string
  /**
   * Undeclared by default, which is the strict reading: only the exact
   * canonical URL is dropped. See {@link RegistryServerType}.
   */
  serverType?: RegistryServerType
  lockfileIncludeTarballUrl?: boolean
}

export function toLockfileResolution (
  pkg: {
    name: string
    version: string
  },
  resolution: Resolution,
  opts: ToLockfileResolutionOptions
): LockfileResolution {
  const { registry, serverType, lockfileIncludeTarballUrl } = opts
  const revision = (resolution as TarballResolution).revision
  if (revision != null && !isValidTarballRevision(revision)) {
    throw new PnpmError('INVALID_TARBALL_REVISION',
      `Cannot serialize invalid tarball revision "${String(revision)}".`)
  }
  if (resolution.type !== undefined) {
    if (revision != null) {
      throw new PnpmError('INVALID_TARBALL_REVISION',
        'Cannot serialize a tarball revision for a non-registry resolution.')
    }
    // Nothing checks a git checkout against a hash — the commit pins the
    // content — so an `integrity` some other tool recorded on a git
    // resolution is dropped rather than written back, instead of standing
    // in the lockfile as a check that never runs.
    if (resolution.type === 'git' && 'integrity' in resolution) {
      const { integrity: _integrity, ...rest } = resolution as GitResolution & { integrity?: string }
      return rest
    }
    return resolution as LockfileResolution
  }
  if (!resolution['integrity']) {
    if (revision != null) {
      throw new PnpmError('INVALID_TARBALL_REVISION',
        'Cannot serialize a tarball revision without integrity.')
    }
    return resolution as LockfileResolution
  }
  // Tarball-typed resolutions are guaranteed to carry a tarball URL by the
  // resolver, but guard for unexpected inputs (e.g. resolutions deserialized
  // from external state) so we don't blow up on a missing field.
  const tarball = resolution['tarball'] as string | undefined
  if (tarball == null) {
    if (revision != null) {
      throw new PnpmError('INVALID_TARBALL_REVISION',
        `Cannot serialize tarball revision ${revision} without its integrity-addressed URL.`)
    }
    return {
      integrity: resolution['integrity'],
    }
  }
  const integrityAddressed = tarball.includes('/-/tarballs/sha512/') &&
    isIntegrityAddressedRegistryTarballUrl(tarball, resolution['integrity'], registry)
  if (revision != null && !integrityAddressed) {
    throw new PnpmError('INVALID_TARBALL_REVISION',
      `Cannot serialize tarball revision ${revision}: its URL does not match its integrity and registry.`)
  }
  if (integrityAddressed) {
    return {
      integrity: resolution['integrity'],
      ...(revision == null ? {} : { revision }),
    }
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
    isCanonicalRegistryTarballUrl(tarball, pkg, { registry, serverType })
  ) {
    return {
      integrity: resolution['integrity'],
    }
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
