import { packageManager } from '@pnpm/cli-meta'

export { getConfig } from './getConfig'
export * from './packageIsInstallable'
export * from './readDepNameCompletions'
export * from './readProjectManifest'
export * from './recursiveSummary'
export * from './style'

export const docsUrl = (cmd: string) => {
  const [pnpmMajorVersion] = packageManager.version.split('.')
  return `https://pnpm.io/${pnpmMajorVersion}.x/cli/${cmd}`
}
