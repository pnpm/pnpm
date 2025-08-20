import { type Config } from './Config.js'

export const DEPS_BUILD_CONFIG_KEYS = [
  'dangerouslyAllowAllBuilds',
  'onlyBuiltDependencies',
  'onlyBuiltDependenciesFile',
  'neverBuiltDependencies',
] as const satisfies Array<keyof Config>

export type DepsBuildConfigKey = typeof DEPS_BUILD_CONFIG_KEYS[number]

export type DepsBuildConfig = Partial<Pick<Config, DepsBuildConfigKey>>

export const hasDependencyBuildOptions = (config: Config): boolean => DEPS_BUILD_CONFIG_KEYS.some(key => config[key] != null)

/**
 * Remove deps build settings from a config.
 * @param targetConfig - Target config object whose deps build settings need to be removed.
 * @returns Record of removed settings.
 */
export function extractAndRemoveDependencyBuildOptions (targetConfig: Config): DepsBuildConfig {
  const depsBuildConfig: DepsBuildConfig = {}
  for (const key of DEPS_BUILD_CONFIG_KEYS) {
    depsBuildConfig[key] = targetConfig[key] as any // eslint-disable-line
    delete targetConfig[key]
  }
  return depsBuildConfig
}
