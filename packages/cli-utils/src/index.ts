import findWorkspaceDir from './findWorkspaceDir'
import getConfig from './getConfig'
import getSaveType from './getSaveType'
import packageManager from './pnpmPkgJson'

export { findWorkspaceDir, getConfig, getSaveType, packageManager }

export * from './createLatestManifestGetter'
export * from './getPinnedVersion'
export * from './packageIsInstallable'
export * from './readImporterManifest'
export * from './style'
export * from './updateToLatestSpecsFromManifest'

export const docsUrl = (cmd: string) => `https://pnpm.js.org/en/cli/${cmd}`
