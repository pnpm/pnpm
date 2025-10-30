import camelcase from 'camelcase'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import { censorProtectedSettings } from './protectedSettings.js'

const shouldChangeCase = (key: string): boolean => key[0] !== '@' && !key.startsWith('//')

function camelCaseConfig (rawConfig: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  for (const key in rawConfig) {
    const targetKey = shouldChangeCase(key) ? camelcase(key) : key
    result[targetKey] = rawConfig[key]
  }
  return result
}

export interface ProcessConfigOptions {
  json?: boolean
}

export function processConfig (rawConfig: Record<string, unknown>): Record<string, unknown> {
  return camelCaseConfig(censorProtectedSettings(sortDirectKeys(rawConfig)))
}
