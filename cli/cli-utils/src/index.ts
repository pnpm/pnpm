import { packageManager } from '@pnpm/cli-meta'

export { calcPnpmfilePathsOfPluginDeps, getConfig } from './getConfig.js'
export * from './packageIsInstallable.js'
export * from './readDepNameCompletions.js'
export * from './readProjectManifest.js'
export * from './recursiveSummary.js'
export * from './style.js'

export function docsUrl (cmd: string): string {
  const [pnpmMajorVersion] = packageManager.version.split('.')
  return `https://pnpm.io/${pnpmMajorVersion}.x/cli/${cmd}`
}
