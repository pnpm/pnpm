import { PnpmError } from '@pnpm/error'

import { assertString, renderReceivedType } from './settingAssertions.js'

/** Null proxy settings leave lower-priority configuration values in place. */
export function normalizeProxySettings (
  settings: { httpProxy?: string, httpsProxy?: string, noProxy?: string | boolean }
): void {
  for (const key of ['httpProxy', 'httpsProxy', 'noProxy'] as const) {
    const value = settings[key]
    if (value == null) {
      delete settings[key]
    } else if (key !== 'noProxy') {
      assertString(value, key)
    } else if (typeof value !== 'string' && typeof value !== 'boolean') {
      throw new PnpmError('INVALID_SETTING', `The "noProxy" setting should be a boolean or string, but got ${renderReceivedType(value)}`)
    }
  }
}
