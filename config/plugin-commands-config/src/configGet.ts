import type { Config, ConfigCommandOptions } from '@pnpm/types'

export function configGet(opts: ConfigCommandOptions, key: string): Config | string {
  const config = opts.rawConfig[key]

  return typeof config === 'boolean' ? config.toString() : config
}
