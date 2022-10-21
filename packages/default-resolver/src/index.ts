import { PnpmError } from '@pnpm/error'
import { FetchFromRegistry, GetAuthHeader } from '@pnpm/fetching-types'
import { createGitResolver } from '@pnpm/git-resolver'
import { resolveFromLocal } from '@pnpm/local-resolver'
import {
  createNpmResolver,
  PackageMeta,
  PackageMetaCache,
  ResolveFromNpmOptions,
  ResolverFactoryOptions,
} from '@pnpm/npm-resolver'
import { ResolveFunction } from '@pnpm/resolver-base'
import { resolveFromTarball } from '@pnpm/tarball-resolver'

export {
  PackageMeta,
  PackageMetaCache,
  ResolveFunction,
  ResolverFactoryOptions,
}

export function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions
): ResolveFunction {
  const resolveFromNpm = createNpmResolver(fetchFromRegistry, getAuthHeader, pnpmOpts)
  const resolveFromGit = createGitResolver(pnpmOpts)
  return async (wantedDependency, opts) => {
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
  }
}
