import { processConfig } from './processConfig.js'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

export type ConfigListOptions = Omit<ConfigCommandOptions, 'json'>

export async function configList (opts: ConfigListOptions): Promise<string> {
  const processedConfig = processConfig(opts.rawConfig)
  return JSON.stringify(processedConfig, undefined, 2)
}
