import PnpmError from '@pnpm/error'
import { FetchFromRegistry, GetCredentials } from '@pnpm/fetching-types'
import createResolveFromGit from '@pnpm/git-resolver'
import resolveFromLocal from '@pnpm/local-resolver'
import createResolveFromNpm, {
  PackageMeta,
  PackageMetaCache,
  ResolveFromNpmOptions,
  ResolverFactoryOptions,
} from '@pnpm/npm-resolver'
import { ResolveFunction } from '@pnpm/resolver-base'
import resolveFromTarball from '@pnpm/tarball-resolver'

export {
  PackageMeta,
  PackageMetaCache,
  ResolveFunction,
  ResolverFactoryOptions,
}

export default function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: GetCredentials,
  pnpmOpts: ResolverFactoryOptions
): ResolveFunction {
  const resolveFromNpm = createResolveFromNpm(fetchFromRegistry, getCredentials, pnpmOpts)
  const resolveFromGit = createResolveFromGit(pnpmOpts)
  return async (wantedDependency, opts) => {
    const resolution = await resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions) ??
      (wantedDependency.pref && (
        await resolveFromTarball(wantedDependency as {pref: string}) ??
        await resolveFromGit(wantedDependency as {pref: string}) ??
        await resolveFromLocal(wantedDependency as {pref: string}, opts)
      ))
    if (!resolution) {
      throw new PnpmError(
        'SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER',
        `${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref ?? ''} isn't supported by any available resolver.`)
    }
    return resolution
  }
}
