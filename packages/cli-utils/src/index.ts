import getConfig from './getConfig'
import getSaveType from './getSaveType'

export { getConfig, getSaveType }

export * from './getOptionType'
export * from './getPinnedVersion'
export * from './optionTypesToCompletions'
export * from './packageIsInstallable'
export * from './readDepNameCompletions'
export * from './readProjectManifest'
export * from './recursiveSummary'
export * from './style'

export const docsUrl = (cmd: string) => `https://pnpm.js.org/en/cli/${cmd}`
