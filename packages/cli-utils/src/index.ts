import getConfig from './getConfig'

export { getConfig }

export * from './packageIsInstallable'
export * from './readDepNameCompletions'
export * from './readProjectManifest'
export * from './recursiveSummary'
export * from './style'

export const docsUrl = (cmd: string) => `https://pnpm.js.org/en/cli/${cmd}`
