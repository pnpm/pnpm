import url from 'url'
import { type PackageSnapshot, type TarballResolution } from '@pnpm/lockfile.types'
import { type Resolution } from '@pnpm/resolver-base'
import { type Registries } from '@pnpm/types'
import { type ResolverPlugin } from '@pnpm/hooks.types'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries
): Resolution {
  if (
    Boolean((pkgSnapshot.resolution as TarballResolution).type) ||
    (pkgSnapshot.resolution as TarballResolution).tarball?.startsWith('file:') ||
    isGitHostedPkgUrl((pkgSnapshot.resolution as TarballResolution).tarball ?? '')
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

export async function pkgSnapshotToResolutionWithResolvers (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  registries: Registries,
  opts: {
    customResolvers?: ResolverPlugin[]
    lockfileDir: string
    projectDir: string
  }
): Promise<Resolution> {
  if (opts.customResolvers) {
    for (const resolver of opts.customResolvers) {
      if (resolver.supportsLockfileResolution) {
        const supportsResult = resolver.supportsLockfileResolution(depPath, pkgSnapshot.resolution)
        // eslint-disable-next-line no-await-in-loop
        const supports = supportsResult instanceof Promise ? await supportsResult : supportsResult
        if (supports && resolver.fromLockfileResolution) {
          const resolutionResult = resolver.fromLockfileResolution(depPath, pkgSnapshot.resolution, {
            lockfileDir: opts.lockfileDir,
            projectDir: opts.projectDir,
            preferredVersions: {},
          })
          // eslint-disable-next-line no-await-in-loop
          return (resolutionResult instanceof Promise ? await resolutionResult : resolutionResult) as Resolution
        }
      }
    }
  }

  return pkgSnapshotToResolution(depPath, pkgSnapshot, registries)
}
