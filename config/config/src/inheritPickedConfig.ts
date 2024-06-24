import { type Config } from './Config'

export type InheritableConfig = Partial<Config> & Pick<Config, 'rawConfig' | 'rawLocalConfig'>
export type PickConfig = (cfg: Partial<Config>) => Partial<Config>
export type PickRawConfig = (cfg: Record<string, unknown>) => Record<string, unknown>

export function inheritPickedConfig (
  targetCfg: InheritableConfig,
  srcCfg: InheritableConfig,
  pickConfig: PickConfig,
  pickRawConfig: PickRawConfig,
  pickRawLocalConfig: PickRawConfig = pickRawConfig
): void {
  Object.assign(targetCfg, pickConfig(srcCfg))
  Object.assign(targetCfg.rawConfig, pickRawConfig(srcCfg.rawConfig))
  Object.assign(targetCfg.rawLocalConfig, pickRawLocalConfig(srcCfg.rawLocalConfig))
}
