import type { LockfileResolution } from '@pnpm/lockfile.types'
import { type GitResolution, isGitHostedTarballUrl, type Resolution, type TarballResolution } from '@pnpm/resolving.resolver-base'
import { isCanonicalRegistryTarballUrl } from '@pnpm/resolving.tarball-url'
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
  if (resolution.type !== undefined) {
    return serializeNonTarballResolution(resolution)
  }
  const integrity = resolution['integrity']
  if (!integrity) {
    return resolution as LockfileResolution
  }
  const tarball = resolution.tarball
  if (tarball == null) {
    return { integrity }
  }
  return serializeStandardTarball(pkg, resolution, tarball, integrity, opts)
}

function serializeNonTarballResolution (resolution: Resolution): LockfileResolution {
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

function serializeStandardTarball (
  pkg: { name: string, version: string },
  resolution: Resolution,
  tarball: string,
  integrity: string,
  opts: ToLockfileResolutionOptions
): LockfileResolution {
  const gitHosted = (resolution as TarballResolution).gitHosted === true ||
    isGitHostedTarballUrl(tarball)
  if (
    !opts.lockfileIncludeTarballUrl &&
    !gitHosted &&
    !tarball.startsWith('file:') &&
    isCanonicalRegistryTarballUrl(tarball, pkg, { registry: opts.registry, serverType: opts.serverType })
  ) {
    return { integrity }
  }
  const { path } = resolution as TarballResolution
  return {
    integrity,
    tarball,
    ...(gitHosted ? { gitHosted: true } : {}),
    ...(path == null ? {} : { path }),
  }
}
