import kebabCase from 'lodash.kebabcase'
import { encode } from 'ini'
import { types } from '@pnpm/config'
import { isCamelCase, isStrictlyKebabCase } from '@pnpm/naming-cases'
import { getObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'
import { normalizeConfigKeyCases } from './configKeyCases.js'
import { parseConfigPropertyPath } from './parseConfigPropertyPath.js'
import { settingShouldFallBackToNpm } from './settingShouldFallBackToNpm.js'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key])
    return { output: '', exitCode: exitCode ?? 0 }
  }
  const configResult = getRcConfig(opts.rawConfig, key) ?? getConfigByPropertyPath(opts.rawConfig, key)
  const output = displayConfig(configResult?.value, opts)
  return { output, exitCode: 0 }
}

interface Found<Value> {
  value: Value
}

function getRcConfig (rawConfig: Record<string, unknown>, key: string): Found<unknown> | undefined {
  const rcKey = isCamelCase(key) ? kebabCase(key) : key
  if (rcKey in types) {
    const value = rawConfig[rcKey]
    return { value }
  }
  if (isStrictlyKebabCase(key)) {
    return { value: undefined }
  }
  return undefined
}

type GetConfigByPropertyPathOptions = Pick<ConfigCommandOptions, 'json'>

function getConfigByPropertyPath (rawConfig: Record<string, unknown>, propertyPath: string, opts?: GetConfigByPropertyPathOptions): Found<unknown> {
  const parsedPropertyPath = Array.from(parseConfigPropertyPath(propertyPath))
  if (parsedPropertyPath.length === 0) {
    return {
      value: normalizeConfigKeyCases(rawConfig, opts),
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
    return encode(config)
  }
  return String(config)
}
