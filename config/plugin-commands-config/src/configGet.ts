import type { Config } from '@pnpm/types'
import type { ConfigCommandOptions } from './ConfigCommandOptions'

export function configGet(opts: ConfigCommandOptions, key: string): Config | string {
  const config = opts.rawConfig[key]

  return typeof config === 'boolean' ? config.toString() : config
}
