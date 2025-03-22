import kebabCase from 'lodash.kebabcase'
import { type ConfigCommandOptions } from './ConfigCommandOptions'

export function configGet (opts: ConfigCommandOptions, key: string): string {
  const config = opts.rawConfig[kebabCase(key)]
  return Array.isArray(config) ? config.join(',') : String(config)
}
