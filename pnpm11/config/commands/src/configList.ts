import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { configToRecord } from './configToRecord.js'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  return JSON.stringify(configToRecord(opts._config, opts._context.explicitlySetKeys), undefined, 2)
}
