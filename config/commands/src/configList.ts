import type { Config, ConfigContext } from '@pnpm/config.reader'

import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { configToRecord } from './configToRecord.js'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  const combined = opts as unknown as Config & ConfigContext
  return JSON.stringify(configToRecord(combined, combined), undefined, 2)
}
