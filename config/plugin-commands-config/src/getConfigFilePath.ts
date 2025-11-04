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

  switch (isSupportedNpmConfig(key)) {
  case false:
  case 'compat': {
    const { configDir } = opts
    const configFileName = opts.global ? GLOBAL_CONFIG_YAML_FILENAME : WORKSPACE_MANIFEST_FILENAME
    return { configDir, configFileName }
  }

  case true: {
    let rcName: 'rc' | '.npmrc'
    let yamlName: typeof GLOBAL_CONFIG_YAML_FILENAME | typeof WORKSPACE_MANIFEST_FILENAME
    let configDir: string

    if (opts.global) {
      rcName = 'rc'
      yamlName = GLOBAL_CONFIG_YAML_FILENAME
      configDir = opts.configDir
    } else {
      rcName = '.npmrc'
      yamlName = WORKSPACE_MANIFEST_FILENAME
      configDir = opts.dir
    }

    return fs.existsSync(path.join(configDir, yamlName))
      ? { configDir, configFileName: yamlName }
      : { configDir, configFileName: rcName }
  }
  }
}
