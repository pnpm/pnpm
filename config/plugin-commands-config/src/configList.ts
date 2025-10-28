import { encode } from 'ini'
import { processConfig } from './processConfig.js'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  const processedConfig = processConfig(opts.rawConfig, opts)
  if (opts.json) {
    return JSON.stringify(processedConfig, null, 2)
  }
  return encode(processedConfig)
}
