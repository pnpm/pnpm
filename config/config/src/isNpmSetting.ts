const NPM_AUTH_SETTINGS = [
  '_auth',
  '_authToken',
  '_password',
  'cafile',
  'email',
  'keyfile',
  'key',
  'registry',
  'username',
]

export const isNpmSetting = (key: string): boolean =>
  key.startsWith('@') || key.startsWith('//') || NPM_AUTH_SETTINGS.includes(key)
