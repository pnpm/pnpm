import { type ConfigCommandOptions } from './ConfigCommandOptions'

export function configGet (opts: ConfigCommandOptions, key: string) {
  const config = opts.rawConfig[key]
  return typeof config === 'boolean' ? config.toString() : config
}
