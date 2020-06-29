import createResolver, { ResolveFunction, ResolverFactoryOptions } from '@pnpm/default-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import { DependencyManifest, Registries } from '@pnpm/types'
import getCredentialsByURI = require('credentials-by-uri')
import LRU = require('lru-cache')
import mem = require('mem')

type GetManifestOpts = {
  dir: string,
  lockfileDir: string,
  rawConfig: object,
  registries: Registries,
}

export type ManifestGetterOptions = Omit<ResolverFactoryOptions, 'metaCache'> & GetManifestOpts

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, pref: string) => Promise<DependencyManifest | null> {
  const fetch = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.rawConfig, registry))
  const resolve = createResolver(fetch, getCredentials, Object.assign(opts, {
    metaCache: new LRU({
      max: 10000,
      maxAge: 120 * 1000, // 2 minutes
    }) as any, // tslint:disable-line:no-any
  }))
  return getManifest.bind(null, resolve, opts)
}

export async function getManifest (
  resolve: ResolveFunction,
  opts: GetManifestOpts,
  packageName: string,
  pref: string
) {
  const resolution = await resolve({ alias: packageName, pref }, {
    lockfileDir: opts.lockfileDir,
    preferredVersions: {},
    projectDir: opts.dir,
    registry: pickRegistryForPackage(opts.registries, packageName),
  })
  return resolution?.manifest ?? null
}
