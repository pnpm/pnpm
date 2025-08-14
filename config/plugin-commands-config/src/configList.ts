import { encode } from 'ini'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import { type ConfigCommandOptions } from './ConfigCommandOptions'
import { censorProtectedSettings } from './protectedSettings'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  const sortedConfig = censorProtectedSettings(sortDirectKeys(opts.rawConfig))
  if (opts.json) {
    return JSON.stringify(sortedConfig, null, 2)
  }
  return encode(sortedConfig)
}
