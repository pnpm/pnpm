import type { Config } from '@pnpm/config.reader'
import type { Registries } from '@pnpm/types'

const DEFAULT_PACKAGE_MANAGER_REGISTRY = 'https://registry.npmjs.org/'

export function getPackageManagerRegistries (config: Config): Registries {
  return {
    default: DEFAULT_PACKAGE_MANAGER_REGISTRY,
    ...config.packageManagerRegistries,
  }
}
