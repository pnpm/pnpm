import type { Registries, RegistryConfig } from '@pnpm/types'

import type { Config } from './Config.js'

type PackageManagerConfig = Pick<Config, 'packageManagerRegistries' | 'packageManagerNetworkConfig'>

const DEFAULT_PACKAGE_MANAGER_REGISTRY = 'https://registry.npmjs.org/'

export interface PackageManagerBootstrapConfig {
  ca?: string | string[]
  cert?: string | string[]
  configByUri: Record<string, RegistryConfig>
  httpProxy?: string
  httpsProxy?: string
  key?: string
  localAddress?: string
  noProxy?: string | boolean
  registries: Registries
  strictSsl?: boolean
}

export function getPackageManagerRegistries (config: PackageManagerConfig): Registries {
  return {
    default: DEFAULT_PACKAGE_MANAGER_REGISTRY,
    ...config.packageManagerRegistries,
  }
}

export function getPackageManagerBootstrapConfig (config: PackageManagerConfig): PackageManagerBootstrapConfig {
  return {
    ca: config.packageManagerNetworkConfig?.ca,
    cert: config.packageManagerNetworkConfig?.cert,
    configByUri: config.packageManagerNetworkConfig?.configByUri ?? {},
    httpProxy: config.packageManagerNetworkConfig?.httpProxy,
    httpsProxy: config.packageManagerNetworkConfig?.httpsProxy,
    key: config.packageManagerNetworkConfig?.key,
    localAddress: config.packageManagerNetworkConfig?.localAddress,
    noProxy: config.packageManagerNetworkConfig?.noProxy,
    registries: getPackageManagerRegistries(config),
    strictSsl: config.packageManagerNetworkConfig?.strictSsl,
  }
}
