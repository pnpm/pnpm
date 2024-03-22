import '@total-typescript/ts-reset'
import { PnpmError } from '@pnpm/error'
import type {
  PackageMeta,
  GetAuthHeader,
  ResolveFunction,
  PackageMetaCache,
  WantedDependency,
  FetchFromRegistry,
  ResolveResult,
  ResolveFromNpmOptions,
  ResolverFactoryOptions,
} from '@pnpm/types'
import { createGitResolver } from '@pnpm/git-resolver'
import { resolveFromLocal } from '@pnpm/local-resolver'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { resolveFromTarball } from '@pnpm/tarball-resolver'

export type {
  PackageMeta,
  PackageMetaCache,
  ResolveFunction,
  ResolverFactoryOptions,
}

export function createResolver(
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions
): ResolveFunction {
  const resolveFromNpm = createNpmResolver(
    fetchFromRegistry,
    getAuthHeader,
    pnpmOpts
  )

  const resolveFromGit = createGitResolver(pnpmOpts)

  return async (wantedDependency: WantedDependency, opts: ResolveFromNpmOptions) => {
    const resolution: ResolveResult | null =
      (await resolveFromNpm(wantedDependency, opts)) ??
      (wantedDependency.pref &&
        ((await resolveFromTarball(wantedDependency)) ??
          (await resolveFromGit(wantedDependency)) ??
          (await resolveFromLocal(wantedDependency, opts))))

    if (!resolution) {
      throw new PnpmError(
        'SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER',
        `${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref ?? ''} isn't supported by any available resolver.`
      )
    }

    return resolution
  }
}
