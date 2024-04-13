import { type ConfigCommandOptions } from './ConfigCommandOptions'

export function configGet (opts: ConfigCommandOptions, key: string): string {
  const config = opts.rawConfig[key]
  return String(config)
}
