import { type Config, isIniConfigKey, types } from '@pnpm/config.reader'
import { getObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { isCamelCase, isStrictlyKebabCase } from '@pnpm/text.naming-cases'
import camelcase from 'camelcase'
import kebabCase from 'lodash.kebabcase'

import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { configToRecord } from './configToRecord.js'
import { parseConfigPropertyPath } from './parseConfigPropertyPath.js'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  const isScopedKey = key.startsWith('@')
  const configResult = lookupConfig(opts, key, isScopedKey) ?? lookupByPropertyPath(opts, key)
  const output = displayConfig(configResult?.value, opts)
  return { output, exitCode: 0 }
}

interface Found<Value> {
  value: Value
}

function lookupConfig (opts: ConfigCommandOptions, key: string, isScopedKey: boolean): Found<unknown> | undefined {
  if (isScopedKey || isIniConfigKey(key)) {
    return { value: opts.authConfig[key] }
  }
  const kebabKey = isCamelCase(key) ? kebabCase(key) : key
  if (Object.hasOwn(types, kebabKey)) {
    const camelKey = camelcase(kebabKey, { locale: 'en-US' })
    return { value: (opts as unknown as Record<string, unknown>)[camelKey] }
  }
  if (isStrictlyKebabCase(key)) {
    const camelKey = camelcase(key, { locale: 'en-US' })
    return { value: (opts as unknown as Record<string, unknown>)[camelKey] }
  }
  return undefined
}

function lookupByPropertyPath (opts: ConfigCommandOptions, propertyPath: string): Found<unknown> {
  const parsedPropertyPath = Array.from(parseConfigPropertyPath(propertyPath))
  if (parsedPropertyPath.length === 0) {
    return { value: configToRecord(opts as unknown as Config) }
  }
  const record = configToRecord(opts as unknown as Config)
  return {
    value: getObjectValueByPropertyPath(record, parsedPropertyPath),
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
