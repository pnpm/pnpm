import type { Config } from '@pnpm/config.reader'

import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { configToRecord } from './configToRecord.js'

export type ConfigListOptions = Pick<ConfigCommandOptions, 'authConfig'>

export async function configList (opts: ConfigListOptions): Promise<string> {
  return JSON.stringify(configToRecord(opts as unknown as Config), undefined, 2)
}
