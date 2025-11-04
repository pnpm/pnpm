import fs from 'fs'
import path from 'path'
import kebabCase from 'lodash.kebabcase'
import { isSupportedNpmConfig } from '@pnpm/config'
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

export function getConfigFilePath (key: string, opts: Pick<ConfigCommandOptions, 'global' | 'configDir' | 'dir'>): ConfigFilePathInfo {
  key = kebabCase(key)

  const configDir = opts.global ? opts.configDir : opts.dir

  switch (isSupportedNpmConfig(key)) {
  case false:
  case 'compat': {
    const configFileName = opts.global ? GLOBAL_CONFIG_YAML_FILENAME : WORKSPACE_MANIFEST_FILENAME
    return { configDir, configFileName }
  }

  case true: {
    // NOTE: The following code no longer does what the merged PR at <https://github.com/pnpm/pnpm/pull/10073> wants to do,
    //       but considering the settings are now clearly divided into 2 separate categories, it should no longer be relevant.
    // TODO: Maybe pnpm should not load npm-compatible settings from the yaml file?
    // TODO: Alternatively, only set npm-compatible settings to the yaml file if the setting is found there.
    const configFileName = opts.global ? 'rc' : '.npmrc'
    return { configDir, configFileName }
  }
  }
}
