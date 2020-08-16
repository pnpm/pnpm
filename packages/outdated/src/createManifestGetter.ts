import createResolver, { ResolveFunction, ResolverFactoryOptions } from '@pnpm/default-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import { DependencyManifest, Registries } from '@pnpm/types'
import getCredentialsByURI = require('credentials-by-uri')
import mem = require('mem')

type GetManifestOpts = {
  dir: string,
  lockfileDir: string,
  rawConfig: object,
  registries: Registries,
}

export type ManifestGetterOptions = ResolverFactoryOptions & GetManifestOpts

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, pref: string) => Promise<DependencyManifest | null> {
  const fetch = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.rawConfig, registry))
  const resolve = createResolver(fetch, getCredentials, opts)
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
