import packageManager from './pnpmPkgJson'

export { packageManager }

export * from './createLatestManifestGetter'
export * from './packageIsInstallable'
export * from './readImporterManifest'
export * from './style'

export const docsUrl = (cmd: string) => `https://pnpm.js.org/en/cli/${cmd}`
