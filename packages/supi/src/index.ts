// Patch the global fs module here at the app level
import './fs/gracefulify'

export { PackageManifest, PnpmOptions } from '@pnpm/types'
export * from './api'
export { PnpmError, PnpmErrorCode } from './errorTypes'

export { ImportersOptions } from './getContext'
export { InstallOptions } from './install/extendInstallOptions'
export { RebuildOptions } from './rebuild/extendRebuildOptions'
export { UninstallOptions } from './uninstall/extendUninstallOptions'

export { LocalPackages } from '@pnpm/resolver-base'
