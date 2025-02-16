import { encode } from 'ini'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import { type ConfigCommandOptions } from './ConfigCommandOptions'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  const sortedConfig = sortDirectKeys(opts.rawConfig)
  if (opts.json) {
    return JSON.stringify(sortedConfig, null, 2)
  }
  return encode(sortedConfig)
}
