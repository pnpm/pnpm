import pnpmManifest from '@pnpm/cli-meta'
import getConfig from './getConfig'

export { getConfig }

export * from './packageIsInstallable'
export * from './readDepNameCompletions'
export * from './readProjectManifest'
export * from './recursiveSummary'
export * from './style'

export const docsUrl = (cmd: string) => {
  const [pnpmMajorVersion] = pnpmManifest.version.split('.')
  return `https://pnpm.js.org/${pnpmMajorVersion}.x/cli/${cmd}`
}
