import type { Config } from './Config.js'

export interface InheritableConfigPair {
  config: Partial<Config> & Pick<Config, 'authConfig'>
}
export type PickConfig = (cfg: Partial<Config>) => Partial<Config>
export type PickRawConfig = (cfg: Record<string, unknown>) => Record<string, unknown>

export function inheritPickedConfig (
  target: InheritableConfigPair,
  src: InheritableConfigPair,
  pickConfig: PickConfig,
  pickRawConfig: PickRawConfig
): void {
  Object.assign(target.config, pickConfig(src.config))
  Object.assign(target.config.authConfig, pickRawConfig(src.config.authConfig))
}
