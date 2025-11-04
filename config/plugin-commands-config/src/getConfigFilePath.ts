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
  configFileName: ConfigFileName
  configPath: string
}

/**
 * Priority: pnpm-workspace.yaml > .npmrc > default to pnpm-workspace.yaml
 */
export function getConfigFilePath (key: string, opts: Pick<ConfigCommandOptions, 'global' | 'configDir' | 'dir'>): ConfigFilePathInfo {
  key = kebabCase(key)

  switch (isSupportedNpmConfig(key)) {
  case false:
  case 'compat': {
    const configFileName = opts.global ? GLOBAL_CONFIG_YAML_FILENAME : WORKSPACE_MANIFEST_FILENAME
    const configPath = path.join(opts.configDir, configFileName)
    return { configFileName, configPath }
  }

  case true: {
    let rcName: 'rc' | '.npmrc'
    let yamlName: typeof GLOBAL_CONFIG_YAML_FILENAME | typeof WORKSPACE_MANIFEST_FILENAME
    let dir: string

    if (opts.global) {
      rcName = 'rc'
      yamlName = GLOBAL_CONFIG_YAML_FILENAME
      dir = opts.configDir
    } else {
      rcName = '.npmrc'
      yamlName = WORKSPACE_MANIFEST_FILENAME
      dir = opts.dir
    }

    const rcPath = path.join(dir, rcName)
    const yamlPath = path.join(dir, yamlName)
    return fs.existsSync(yamlPath)
      ? { configFileName: yamlName, configPath: yamlPath }
      : { configFileName: rcName, configPath: rcPath }
  }
  }
}
