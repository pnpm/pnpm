import { type Config, isIniConfigKey, types } from '@pnpm/config.reader'
import { getObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { isCamelCase } from '@pnpm/text.naming-cases'
import camelcase from 'camelcase'
import kebabCase from 'lodash.kebabcase'

import type { ConfigCommandOptions } from './ConfigCommandOptions.js'
import { configToRecord } from './configToRecord.js'
import { parseConfigPropertyPath } from './parseConfigPropertyPath.js'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  const isScopedKey = key.startsWith('@')
  const configResult = lookupConfig(opts, key, isScopedKey) ?? (isPropertyPath(key) ? lookupByPropertyPath(opts, key) : { value: undefined })
  const output = displayConfig(configResult?.value, opts)
  return { output, exitCode: 0 }
}

interface Found<Value> {
  value: Value
}

function lookupConfig (opts: ConfigCommandOptions, key: string, isScopedKey: boolean): Found<unknown> | undefined {
  if (isScopedKey) {
    return { value: opts.authConfig[key] }
  }
  const kebabKey = isCamelCase(key) ? kebabCase(key) : key
  // Resolve typed keys (including INI keys like registry, ca, proxy) from Config
  if (Object.hasOwn(types, kebabKey)) {
    const camelKey = camelcase(kebabKey, { locale: 'en-US' })
    const explicit = (opts as unknown as Config).explicitlySetKeys
    // If explicitlySetKeys is available, only return explicitly set values
    if (explicit && !explicit.has(camelKey)) {
      return { value: undefined }
    }
    return { value: (opts as unknown as Record<string, unknown>)[camelKey] }
  }
  // Auth-specific INI keys (//host:_authToken, _auth, etc.) from authConfig
  if (isIniConfigKey(key)) {
    return { value: opts.authConfig[key] }
  }
  // For keys not in types (e.g., package-extensions), look up via configToRecord
  // which excludes internal/sensitive fields.
  const camelKey = camelcase(key, { locale: 'en-US' })
  const record = configToRecord(opts as unknown as Config)
  if (Object.hasOwn(record, camelKey)) {
    return { value: record[camelKey] }
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

function isPropertyPath (key: string): boolean {
  return key === '' || key.includes('.') || key.includes('[')
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
