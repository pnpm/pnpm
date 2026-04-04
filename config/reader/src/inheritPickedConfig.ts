import type { Config, ConfigContext } from './Config.js'

export interface InheritableConfigPair {
  config: Partial<Config> & Pick<Config, 'authConfig'>
  context: Pick<ConfigContext, 'rawLocalConfig'>
}
export type PickConfig = (cfg: Partial<Config>) => Partial<Config>
export type PickRawConfig = (cfg: Record<string, unknown>) => Record<string, unknown>

export function inheritPickedConfig (
  target: InheritableConfigPair,
  src: InheritableConfigPair,
  pickConfig: PickConfig,
  pickRawConfig: PickRawConfig,
  pickRawLocalConfig: PickRawConfig = pickRawConfig
): void {
  Object.assign(target.config, pickConfig(src.config))
  Object.assign(target.config.authConfig, pickRawConfig(src.config.authConfig))
  Object.assign(target.context.rawLocalConfig, pickRawLocalConfig(src.context.rawLocalConfig))
}
