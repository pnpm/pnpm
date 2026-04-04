import { sortDirectKeys } from '@pnpm/object.key-sorting'
import camelcase from 'camelcase'

import { censorProtectedSettings } from './protectedSettings.js'

const shouldChangeCase = (key: string): boolean => key[0] !== '@' && !key.startsWith('//')

function camelCaseConfig (config: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  for (const key in config) {
    const targetKey = shouldChangeCase(key) ? camelcase(key) : key
    result[targetKey] = config[key]
  }
  return result
}

export interface ProcessConfigOptions {
  json?: boolean
}

export function processConfig (config: Record<string, unknown>): Record<string, unknown> {
  return camelCaseConfig(censorProtectedSettings(sortDirectKeys(config)))
}
