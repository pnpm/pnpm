import createResolveFromGit from '@pnpm/git-resolver'
import resolveFromLocal from '@pnpm/local-resolver'
import createResolveFromNpm from '@pnpm/npm-resolver'
import resolveFromTarball from '@pnpm/tarball-resolver'
import {PnpmOptions} from '@pnpm/types'

export default function createResolver (
  pnpmOpts: PnpmOptions & {
    rawNpmConfig: object,
    metaCache: Map<string, object>,
    store: string,
  },
) {
  const resolveFromNpm = createResolveFromNpm(pnpmOpts)
  const resolveFromGit = createResolveFromGit(pnpmOpts)
  return async (wantedDependency: {alias?: string, pref: string}, opts: { registry: string, prefix: string }) => {
    const resolution = await resolveFromNpm(wantedDependency, opts)
      || await resolveFromTarball(wantedDependency)
      || await resolveFromGit(wantedDependency)
      || await resolveFromLocal(wantedDependency, opts)
    if (!resolution) {
      throw new Error(`Cannot resolve ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref} packages not supported`)
    }
    return resolution
  }
}
