import { isIniConfigKey, types } from '@pnpm/config.reader'
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
    // Scoped registry keys (e.g. `@scope:registry`) can be set in two places:
    // `.npmrc` (which lands in authConfig) or pnpm-workspace.yaml's
    // `registries` block (which lands in the merged Config.registries map).
    // Prefer the merged map so this command reports the same value that
    // `pnpm publish` and the resolvers actually use.
    if (key.endsWith(':registry')) {
      const scope = key.slice(0, key.length - ':registry'.length)
      const merged = opts._config.registries?.[scope]
      if (merged !== undefined) {
        return { value: merged }
      }
    }
    return { value: opts.authConfig[key] }
  }
  const kebabKey = isCamelCase(key) ? kebabCase(key) : key
  // Resolve typed keys from Config — check explicitly set values first,
  // then fall back to authConfig (for keys like registry set in .npmrc)
  if (Object.hasOwn(types, kebabKey)) {
    const camelKey = camelcase(kebabKey, { locale: 'en-US' })
    const explicit = opts._context.explicitlySetKeys
    if (!explicit || explicit.has(camelKey)) {
      return { value: (opts._config as unknown as Record<string, unknown>)[camelKey] }
    }
    // Fall back to authConfig for INI keys (registry, ca, etc.)
    if (kebabKey in opts.authConfig) {
      return { value: opts.authConfig[kebabKey] }
    }
    return { value: undefined }
  }
  // Auth-specific INI keys (//host:_authToken, _auth, etc.) from authConfig
  if (isIniConfigKey(key)) {
    return { value: opts.authConfig[key] }
  }
  // For keys not in types (e.g., package-extensions), look up via configToRecord
  // which excludes internal/sensitive fields.
  const camelKey = camelcase(key, { locale: 'en-US' })
  const record = configToRecord(opts._config, opts._context.explicitlySetKeys)
  if (Object.hasOwn(record, camelKey)) {
    return { value: record[camelKey] }
  }
  return undefined
}

function lookupByPropertyPath (opts: ConfigCommandOptions, propertyPath: string): Found<unknown> {
  const parsedPropertyPath = Array.from(parseConfigPropertyPath(propertyPath))
  if (parsedPropertyPath.length === 0) {
    return { value: configToRecord(opts._config, opts._context.explicitlySetKeys) }
  }
  const record = configToRecord(opts._config, opts._context.explicitlySetKeys)
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
