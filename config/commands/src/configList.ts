import type { Config } from '@pnpm/config.reader'

import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { configToRecord } from './configToRecord.js'

export async function configList (opts: ConfigCommandOptions): Promise<string> {
  return JSON.stringify(configToRecord(opts as unknown as Config), undefined, 2)
}
