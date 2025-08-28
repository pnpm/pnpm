import { encode } from 'ini'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import { censorProtectedSettings } from './protectedSettings.js'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  const sortedConfig = censorProtectedSettings(sortDirectKeys(opts.rawConfig))
  if (opts.json) {
    return JSON.stringify(sortedConfig, null, 2)
  }
  return encode(sortedConfig)
}
