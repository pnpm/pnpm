import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { configToRecord } from './configToRecord.js'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  return JSON.stringify(configToRecord(opts.config, opts.context.explicitlySetKeys), undefined, 2)
}
