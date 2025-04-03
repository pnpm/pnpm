import {
  type ClientOptions,
  createResolver,
  type ResolveFunction,
} from '@pnpm/client'
import { type DependencyManifest } from '@pnpm/types'

interface GetManifestOpts {
  dir: string
  lockfileDir: string
  rawConfig: object
}

export type ManifestGetterOptions = Omit<ClientOptions, 'authConfig'>
& GetManifestOpts
& { fullMetadata: boolean, rawConfig: Record<string, string> }

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, pref: string) => Promise<DependencyManifest | null> {
  const { resolve } = createResolver({ ...opts, authConfig: opts.rawConfig })
  return getManifest.bind(null, resolve, opts)
}

export async function getManifest (
  resolve: ResolveFunction,
  opts: GetManifestOpts,
  packageName: string,
  pref: string
): Promise<DependencyManifest | null> {
  const resolution = await resolve({ alias: packageName, pref }, {
    lockfileDir: opts.lockfileDir,
    preferredVersions: {},
    projectDir: opts.dir,
  })
  return resolution?.manifest ?? null
}
