import { type ConfigCommandOptions } from './ConfigCommandOptions'

export function configGet (opts: ConfigCommandOptions, key: string) {
  return opts.rawConfig[key]
}
