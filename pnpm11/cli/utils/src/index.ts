import { packageManager } from '@pnpm/cli.meta'

export { getNodeVersionFromEnginesRuntime } from '@pnpm/config.package-is-installable'
export * from './packageIsInstallable.js'
export * from './promptPageSize.js'
export * from './readDepNameCompletions.js'
export * from './readProjectManifest.js'
export * from './recursiveSummary.js'
export * from './style.js'

export function docsUrl (cmd: string): string {
  const [pnpmMajorVersion] = packageManager.version.split('.')
  return `https://pnpm.io/${pnpmMajorVersion}.x/cli/${cmd}`
}
