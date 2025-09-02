import camelcase from 'camelcase'

const shouldChangeCase = (key: string): boolean => key[0] !== '@' && !key.startsWith('//')

export function camelCaseConfig (config: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  for (const key in config) {
    const targetKey = shouldChangeCase(key) ? camelcase(key) : key
    result[targetKey] = config[key]
  }
  return result
}
