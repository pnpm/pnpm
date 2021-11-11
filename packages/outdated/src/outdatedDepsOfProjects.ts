import path from 'path'
import {
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import matcher from '@pnpm/matcher'
import { read as readModulesManifest } from '@pnpm/modules-yaml'
import {
  IncludedDependencies,
  ProjectManifest,
} from '@pnpm/types'
import unnest from 'ramda/src/unnest'
import { createManifestGetter, ManifestGetterOptions } from './createManifestGetter'
import outdated, { OutdatedPackage } from './outdated'
import { createFetchFromRegistry } from '@pnpm/fetch'
import getCredentialsByURI from 'credentials-by-uri'
import mem from 'mem'

export default async function outdatedDepsOfProjects (
  pkgs: Array<{dir: string, manifest: ProjectManifest}>,
  args: string[],
  opts: Omit<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir' | 'storeDir'> & {
    compatible?: boolean
    include: IncludedDependencies
    storeDir?: string
  } & Partial<Pick<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir'>>
): Promise<OutdatedPackage[][]> {
  if (!opts.lockfileDir) {
    return unnest(await Promise.all(
      pkgs.map(async (pkg) =>
        outdatedDepsOfProjects([pkg], args, { ...opts, lockfileDir: pkg.dir })
      )
    ))
  }
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const modules = await readModulesManifest(path.join(lockfileDir, 'node_modules'))
  const virtualStoreDir = modules?.virtualStoreDir ?? path.join(lockfileDir, 'node_modules/.pnpm')
  const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }) ?? currentLockfile
  const getLatestManifest = createManifestGetter({
    ...opts,
    fullMetadata: opts.fullMetadata === true,
    lockfileDir,
  })
  const npmFetch = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI({ registry: opts.registries.default }, registry))
  const { cacheDir, fullMetadata, offline, preferOffline, retry, timeout } = opts
  const resolverOpts = {
    cacheDir,
    fullMetadata,
    offline,
    preferOffline,
    retry,
    timeout,
  }
  return Promise.all(pkgs.map(async ({ dir, manifest }) => {
    const match = (args.length > 0) && matcher(args) || undefined
    return outdated({
      compatible: opts.compatible,
      currentLockfile,
      getLatestManifest,
      include: opts.include,
      lockfileDir,
      manifest,
      match,
      prefix: dir,
      wantedLockfile,
      registry: opts.registries.default,
      npmFetch,
      getCredentials,
      resolverOpts,
    })
  }))
}
