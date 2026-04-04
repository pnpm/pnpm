import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { processConfig } from './processConfig.js'

export type ConfigListOptions = Pick<ConfigCommandOptions, 'effectiveConfig'>

export async function configList (opts: ConfigListOptions): Promise<string> {
  const processedConfig = processConfig(opts.effectiveConfig)
  return JSON.stringify(processedConfig, undefined, 2)
}
