import fs from 'fs'
import path from 'path'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

interface ConfigFilePathInfo {
  configPath: string
  isWorkspaceYaml: boolean
}

/**
 * Priority: pnpm-workspace.yaml > .npmrc > default to pnpm-workspace.yaml
 */
export function getConfigFilePath (opts: Pick<ConfigCommandOptions, 'global' | 'configDir' | 'dir'>): ConfigFilePathInfo {
  if (opts.global) {
    return {
      configPath: path.join(opts.configDir, 'rc'),
      isWorkspaceYaml: false,
    }
  }

  const workspaceYamlPath = path.join(opts.dir, 'pnpm-workspace.yaml')
  if (fs.existsSync(workspaceYamlPath)) {
    return {
      configPath: workspaceYamlPath,
      isWorkspaceYaml: true,
    }
  }

  const npmrcPath = path.join(opts.dir, '.npmrc')
  if (fs.existsSync(npmrcPath)) {
    return {
      configPath: npmrcPath,
      isWorkspaceYaml: false,
    }
  }

  // If neither exists, return pnpm-workspace.yaml
  return {
    configPath: workspaceYamlPath,
    isWorkspaceYaml: true,
  }
}
