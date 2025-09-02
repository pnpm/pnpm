import camelcase from 'camelcase'

const shouldChangeCase = (key: string): boolean => key[0] !== '@' && !key.startsWith('//')

function camelCaseConfig (rawConfig: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  for (const key in rawConfig) {
    const targetKey = shouldChangeCase(key) ? camelcase(key) : key
    result[targetKey] = rawConfig[key]
  }
  return result
}

export interface NormalizeConfigKeyCasesOptions {
  json?: boolean
}

export function normalizeConfigKeyCases (rawConfig: Record<string, unknown>, opts?: NormalizeConfigKeyCasesOptions): Record<string, unknown> {
  return opts?.json ? camelCaseConfig(rawConfig) : rawConfig
}
