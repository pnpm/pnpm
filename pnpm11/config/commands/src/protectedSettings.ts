import { redactUrlForDisplay } from '@pnpm/error'

const PROXY_KEYS = new Set(['proxy', 'httpProxy', 'httpsProxy', 'http-proxy', 'https-proxy'])
const PROTECTED_SUFFICES = [
  '_auth',
  '_authToken',
  'username',
  '_password',
  'key',
]

/** Protected settings are settings which `npm config get` refuses to print. */
export const isSettingProtected = (key: string): boolean =>
  key.startsWith('//')
    ? PROTECTED_SUFFICES.some(suffix => key.endsWith(`:${suffix}`))
    : PROTECTED_SUFFICES.includes(key)

export function censorProtectedSettingValue (key: string, value: unknown): unknown {
  if (isSettingProtected(key)) return '(protected)'
  if (PROXY_KEYS.has(key) && typeof value === 'string' && value.includes('@')) {
    return redactUrlForDisplay(value)
  }
  return value
}

export function censorProtectedSettings (config: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(config).map(([key, value]) => [key, censorProtectedSettingValue(key, value)]))
}
