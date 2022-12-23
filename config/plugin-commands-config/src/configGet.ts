import { ConfigCommandOptions } from './ConfigCommandOptions'

export function configGet (opts: ConfigCommandOptions, key: string) {
  if (opts.global) {
    return opts.rawConfig[key]
  }
  return opts.rawLocalConfig[key]
}
