const PROTECTED_SUFFICES = [
  '_auth',
  '_authToken',
  'username',
  '_password',
]

/** Protected settings are settings which `npm config get` refuses to print. */
export const isSettingProtected = (key: string): boolean =>
  key.startsWith('//')
    ? PROTECTED_SUFFICES.some(suffix => key.endsWith(`:${suffix}`))
    : PROTECTED_SUFFICES.includes(key)

/** Hide all protected settings by setting them to `(protected)`. */
export function censorProtectedSettings (config: Record<string, unknown>): Record<string, unknown> {
  config = { ...config }
  for (const key in config) {
    if (isSettingProtected(key)) {
      config[key] = '(protected)'
    }
  }
  return config
}
