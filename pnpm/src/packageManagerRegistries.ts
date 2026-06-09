import { type Config, type PackageManagerNetworkConfig } from '@pnpm/config'
import { type Registries } from '@pnpm/types'

const DEFAULT_PACKAGE_MANAGER_REGISTRY = 'https://registry.npmjs.org/'

export type PackageManagerBootstrapConfig = Partial<PackageManagerNetworkConfig> & {
  registries: Registries
}

/**
 * The registries used to download and verify a package-manager binary. These
 * are built from trusted config sources only (CLI options, env config, user
 * and global .npmrc), defaulting to the public npm registry — repository
 * config must not steer where pnpm fetches the binary it is about to execute.
 */
export function getPackageManagerRegistries (config: Config): Registries {
  return {
    default: DEFAULT_PACKAGE_MANAGER_REGISTRY,
    ...config.packageManagerRegistries,
  }
}

export function getPackageManagerBootstrapConfig (config: Config): PackageManagerBootstrapConfig {
  return {
    ...config.packageManagerNetworkConfig,
    registries: getPackageManagerRegistries(config),
  }
}
