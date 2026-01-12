import kebabCase from 'lodash.kebabcase'
import { isIniConfigKey } from '@pnpm/config'
import { GLOBAL_CONFIG_YAML_FILENAME, WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

export type ConfigFileName =
  | 'rc'
  | '.npmrc'
  | typeof GLOBAL_CONFIG_YAML_FILENAME
  | typeof WORKSPACE_MANIFEST_FILENAME

export interface ConfigFilePathInfo {
  configDir: string
  configFileName: ConfigFileName
}

export function getConfigFileInfo (key: string, opts: Pick<ConfigCommandOptions, 'global' | 'configDir' | 'dir'>): ConfigFilePathInfo {
  key = kebabCase(key)

  const configDir = opts.global ? opts.configDir : opts.dir

  if (isIniConfigKey(key)) {
    // NOTE: The following code no longer does what the merged PR at <https://github.com/pnpm/pnpm/pull/10073> wants to do,
    //       but considering the settings are now clearly divided into 2 separate categories, it should no longer be relevant.
    // TODO: Auth, network, and proxy settings should belong only to INI files.
    //       Add more settings to `isIniConfigKey` to make it complete.
    const configFileName = opts.global ? 'rc' : '.npmrc'
    return { configDir, configFileName }
  } else {
    const configFileName = opts.global ? GLOBAL_CONFIG_YAML_FILENAME : WORKSPACE_MANIFEST_FILENAME
    return { configDir, configFileName }
  }
}
