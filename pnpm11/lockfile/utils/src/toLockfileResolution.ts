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
  const revision = (resolution as TarballResolution).revision
  validateRevision(revision)
  if (resolution.type !== undefined) {
    return serializeNonTarballResolution(resolution, revision)
  }
  const integrity = resolution['integrity']
  if (!integrity) {
    if (revision != null) {
      throw new PnpmError('INVALID_TARBALL_REVISION',
        'Cannot serialize a tarball revision without integrity.')
    }
    return resolution as LockfileResolution
  }
  return serializeTarballResolution(pkg, resolution, integrity, revision, opts)
}

function validateRevision (revision?: number | string): void {
  if (revision != null && !isValidTarballRevision(revision)) {
    throw new PnpmError('INVALID_TARBALL_REVISION',
      `Cannot serialize invalid tarball revision "${String(revision)}".`)
  }
}

function serializeNonTarballResolution (resolution: Resolution, revision?: number | string): LockfileResolution {
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

function serializeTarballResolution (
  pkg: { name: string, version: string },
  resolution: Resolution,
  integrity: string,
  revision: number | string | undefined,
  opts: ToLockfileResolutionOptions
): LockfileResolution {
  const { registry, serverType, lockfileIncludeTarballUrl } = opts
  const tarballRes = resolution as TarballResolution
  const tarball = tarballRes.tarball
  if (tarball == null) {
    if (revision != null) {
      throw new PnpmError('INVALID_TARBALL_REVISION',
        `Cannot serialize tarball revision ${revision} without its integrity-addressed URL.`)
    }
    return { integrity }
  }
  const integrityAddressed = tarball.includes('/-/tarballs/sha512/') &&
    isIntegrityAddressedRegistryTarballUrl(tarball, integrity, registry)
  if (revision != null && !integrityAddressed) {
    throw new PnpmError('INVALID_TARBALL_REVISION',
      `Cannot serialize tarball revision ${revision}: its URL does not match its integrity and registry.`)
  }
  if (integrityAddressed) {
    return {
      integrity,
      ...(tarballRes.revision == null ? {} : { revision: tarballRes.revision }),
    }
  }
  return serializeStandardTarball(pkg, resolution, tarball, integrity, {
    lockfileIncludeTarballUrl,
    registry,
    serverType,
  })
}

interface StandardTarballOptions {
  lockfileIncludeTarballUrl?: boolean
  registry: string
  serverType?: RegistryServerType
}

function serializeStandardTarball (
  pkg: { name: string, version: string },
  resolution: Resolution,
  tarball: string,
  integrity: string,
  opts: StandardTarballOptions
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
