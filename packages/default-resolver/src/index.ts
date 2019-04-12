import createResolveFromGit from '@pnpm/git-resolver'
import resolveFromLocal from '@pnpm/local-resolver'
import createResolveFromNpm, {
  PackageMeta,
  PackageMetaCache,
  ResolveFromNpmOptions,
} from '@pnpm/npm-resolver'
import {
  ResolveFunction,
  ResolveOptions,
} from '@pnpm/resolver-base'
import resolveFromTarball from '@pnpm/tarball-resolver'

export {
  PackageMeta,
  PackageMetaCache,
}

export default function createResolver (
  pnpmOpts: {
    rawNpmConfig: object,
    metaCache: PackageMetaCache,
    store: string,
    // TODO: export options type from @pnpm/npm-resolver
    cert?: string,
    fullMetadata?: boolean,
    key?: string,
    ca?: string,
    strictSsl?: boolean,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    userAgent?: string,
    offline?: boolean,
    preferOffline?: boolean,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMintimeout?: number,
    fetchRetryMaxtimeout?: number,
  },
): ResolveFunction {
  const resolveFromNpm = createResolveFromNpm(pnpmOpts)
  const resolveFromGit = createResolveFromGit(pnpmOpts)
  return async (
    wantedDependency,
    opts: ResolveOptions,
  ) => {
    const resolution = await resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions)
      || wantedDependency.pref && (
        await resolveFromTarball(wantedDependency as {pref: string})
        || await resolveFromGit(wantedDependency as {pref: string})
        || await resolveFromLocal(wantedDependency as {pref: string}, opts)
      )
    if (!resolution) {
      throw new Error(`Cannot resolve ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref} packages not supported`)
    }
    return resolution
  }
}
