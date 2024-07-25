import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry, type GetAuthHeader } from '@pnpm/fetching-types'
import { createGitResolver } from '@pnpm/git-resolver'
import { resolveFromLocal } from '@pnpm/local-resolver'
import {
  createNpmResolver,
  type PackageMeta,
  type PackageMetaCache,
  type ResolveFromNpmOptions,
  type ResolverFactoryOptions,
} from '@pnpm/npm-resolver'
import { type ResolveFunction } from '@pnpm/resolver-base'
import { resolveFromTarball } from '@pnpm/tarball-resolver'

export type {
  PackageMeta,
  PackageMetaCache,
  ResolveFunction,
  ResolverFactoryOptions,
}

export function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions
): { resolve: ResolveFunction, clearCache: () => void } {
  const { resolveFromNpm, clearCache } = createNpmResolver(fetchFromRegistry, getAuthHeader, pnpmOpts)
  const resolveFromGit = createGitResolver(pnpmOpts)
  return {
    resolve: async (wantedDependency, opts) => {
      const resolution = await resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions) ??
        (wantedDependency.pref && (
          await resolveFromTarball(wantedDependency as { pref: string }) ??
          await resolveFromGit(wantedDependency as { pref: string }) ??
          await resolveFromLocal(wantedDependency as { pref: string }, opts)
        ))
      if (!resolution) {
        throw new PnpmError(
          'SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER',
          `${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref ?? ''} isn't supported by any available resolver.`)
      }
      return resolution
    },
    clearCache,
  }
}
