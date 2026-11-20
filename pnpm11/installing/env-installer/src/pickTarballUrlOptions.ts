import { getRegistryServerType } from '@pnpm/config.normalize-registries'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { RegistriesByScope, RegistryContext, RegistryServerType } from '@pnpm/types'

export interface ConfigDepRegistryOpts {
  registriesByScope: RegistriesByScope
  registryOptionsByUrl?: RegistryContext['registryOptionsByUrl']
}

export interface ConfigDepTarballUrlOptions {
  registry: string
  serverType?: RegistryServerType
}

/** The registry serving `pkgName` and the tarball layout it declares. */
export function pickTarballUrlOptions (opts: ConfigDepRegistryOpts, pkgName: string): ConfigDepTarballUrlOptions {
  const registry = pickRegistryForPackage(opts.registriesByScope, pkgName)
  return { registry, serverType: getRegistryServerType(opts, registry) }
}
