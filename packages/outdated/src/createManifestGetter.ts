import {
  ClientOptions,
  createResolver,
  ResolveFunction,
} from '@pnpm/client'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import { DependencyManifest, Registries } from '@pnpm/types'

interface GetManifestOpts {
  dir: string
  lockfileDir: string
  rawConfig: object
  registries: Registries
}

export type ManifestGetterOptions = Omit<ClientOptions, 'authConfig'>
& GetManifestOpts
& { fullMetadata: boolean, rawConfig: Record<string, string> }

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, pref: string) => Promise<DependencyManifest | null> {
  const resolve = createResolver({ ...opts, authConfig: opts.rawConfig })
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
