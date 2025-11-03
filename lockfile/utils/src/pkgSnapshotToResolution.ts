import url from 'url'
import { type PackageSnapshot, type TarballResolution } from '@pnpm/lockfile.types'
import { type Resolution } from '@pnpm/resolver-base'
import { type Registries } from '@pnpm/types'
import { type ResolverPlugin } from '@pnpm/hooks.types'
import { PnpmError } from '@pnpm/error'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

const STANDARD_RESOLUTION_TYPES = new Set(['directory', 'git', 'binary', 'variations'])

function isCustomResolution (resolution: Resolution): boolean {
  const resolutionType = (resolution as { type?: string }).type
  return resolutionType != null && !STANDARD_RESOLUTION_TYPES.has(resolutionType)
}

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
  // Check if this is a custom resolution type
  if (isCustomResolution(pkgSnapshot.resolution as Resolution)) {
    const resolutionType = (pkgSnapshot.resolution as { type: string }).type

    // Try custom resolvers
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
            const resolution = resolutionResult instanceof Promise ? await resolutionResult : resolutionResult
            return resolution as Resolution
          }
        }
      }
    }

    // No resolver found for custom type
    throw new PnpmError('UNSUPPORTED_LOCKFILE_RESOLUTION',
      `Cannot resolve package "${depPath}" with custom resolution type "${resolutionType}". ` +
      'No custom resolver plugin is available to handle this resolution type. ' +
      `You may need to configure a pnpmfile with a custom resolver that supports "${resolutionType}".`,
      {
        hint: 'Add a custom resolver to your .pnpmfile.cjs:\n\n' +
              'module.exports = {\n' +
              '  hooks: {\n' +
              '    resolvers: [{\n' +
              '      name: \'my-resolver\',\n' +
              '      supportsLockfileResolution: async (pkgId, resolution) => {\n' +
              `        return resolution.type === '${resolutionType}'\n` +
              '      },\n' +
              '      fromLockfileResolution: async (pkgId, resolution, opts) => {\n' +
              '        // Convert custom resolution to standard resolution\n' +
              '        return { tarball: \'...\', integrity: \'...\' }\n' +
              '      }\n' +
              '    }]\n' +
              '  }\n' +
              '}',
      }
    )
  }

  // Try custom resolvers for standard resolutions (they might want to intercept)
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
          const resolution = resolutionResult instanceof Promise ? await resolutionResult : resolutionResult
          return resolution as Resolution
        }
      }
    }
  }

  // Fall back to standard resolution
  return pkgSnapshotToResolution(depPath, pkgSnapshot, registries)
}
