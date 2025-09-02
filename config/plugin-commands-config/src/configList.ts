import { encode } from 'ini'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import { normalizeConfigKeyCases } from './configKeyCases.js'
import { censorProtectedSettings } from './protectedSettings.js'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  const processedConfig = normalizeConfigKeyCases(censorProtectedSettings(sortDirectKeys(opts.rawConfig)), opts)
  if (opts.json) {
    return JSON.stringify(processedConfig, null, 2)
  }
  return encode(processedConfig)
}
