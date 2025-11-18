import path from 'path'
import kebabCase from 'lodash.kebabcase'
import { types } from '@pnpm/config'
import { isCamelCase, isStrictlyKebabCase } from '@pnpm/naming-cases'
import { getObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'
import { processConfig } from './processConfig.js'
import { parseConfigPropertyPath } from './parseConfigPropertyPath.js'
import { settingShouldFallBackToNpm } from './settingShouldFallBackToNpm.js'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  const isScopedKey = key.startsWith('@')
  // Exclude scoped keys from npm fallback because they are pnpm-native config
  // that can be read directly from rawConfig (e.g., '@scope:registry')
  if (opts.global && settingShouldFallBackToNpm(key) && !isScopedKey) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key], {
      location: 'user',
      userConfigPath: path.join(opts.configDir, 'rc'),
    })
    return { output: '', exitCode: exitCode ?? 0 }
  }
  const configResult = getRcConfig(opts.rawConfig, key, isScopedKey) ?? getConfigByPropertyPath(opts.rawConfig, key)
  const output = displayConfig(configResult?.value, opts)
  return { output, exitCode: 0 }
}

interface Found<Value> {
  value: Value
}

function getRcConfig (rawConfig: Record<string, unknown>, key: string, isScopedKey: boolean): Found<unknown> | undefined {
  if (isScopedKey) {
    const value = rawConfig[key]
    return { value }
  }
  const rcKey = isCamelCase(key) ? kebabCase(key) : key
  if (rcKey in types) {
    const value = rawConfig[rcKey]
    return { value }
  }
  if (isStrictlyKebabCase(key)) {
    const value = rawConfig[key]
    return { value }
  }
  return undefined
}

function getConfigByPropertyPath (rawConfig: Record<string, unknown>, propertyPath: string): Found<unknown> {
  const parsedPropertyPath = Array.from(parseConfigPropertyPath(propertyPath))
  if (parsedPropertyPath.length === 0) {
    return {
      value: processConfig(rawConfig),
    }
  }
  return {
    value: getObjectValueByPropertyPath(rawConfig, parsedPropertyPath),
  }
}

type DisplayConfigOptions = Pick<ConfigCommandOptions, 'json'>

function displayConfig (config: unknown, opts: DisplayConfigOptions): string {
  if (Boolean(opts.json) || Array.isArray(config)) {
    return JSON.stringify(config, undefined, 2)
  }
  if (typeof config === 'object' && config != null) {
    return JSON.stringify(config, undefined, 2)
  }
  return String(config)
}
