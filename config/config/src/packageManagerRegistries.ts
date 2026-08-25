import { type Registries } from '@pnpm/types'

import { type Config, type PackageManagerNetworkConfig } from './Config.js'

const DEFAULT_PACKAGE_MANAGER_REGISTRY = 'https://registry.npmjs.org/'

type PackageManagerConfig = Pick<Config, 'packageManagerNetworkConfig' | 'packageManagerRegistries'>

export type PackageManagerBootstrapConfig = PackageManagerNetworkConfig & {
  registries: Registries
}

/**
 * The registries used to download and verify a package-manager binary. These
 * are built from trusted config sources only (CLI options, env config, user
 * and global .npmrc), defaulting to the public npm registry — repository
 * config must not steer where pnpm fetches the binary it is about to execute.
 */
export function getPackageManagerBootstrapConfig (config: PackageManagerConfig): PackageManagerBootstrapConfig {
  return {
    rawConfig: {},
    sslConfigs: {},
    ...config.packageManagerNetworkConfig,
    registries: getPackageManagerRegistries(config),
  }
}

export function getPackageManagerRegistries (config: PackageManagerConfig): Registries {
  return {
    default: DEFAULT_PACKAGE_MANAGER_REGISTRY,
    ...config.packageManagerRegistries,
  }
}
