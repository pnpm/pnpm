import { type Config, types } from '@pnpm/config.reader'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import camelcase from 'camelcase'

import { censorProtectedSettings } from './protectedSettings.js'

// Auth-related Config fields that are internal objects, not user settings.
const NON_SETTING_CONFIG_KEYS = new Set([
  'authConfig', 'credsByUri',
])

/**
 * Convert a Config object to a camelCase record for display.
 * Only includes explicitly set values (from CLI, env vars, or workspace yaml),
 * not default values. Auth/registry keys from authConfig are always included.
 *
 * Accepts a clean Config object (without ConfigContext fields mixed in),
 * so no INTERNAL_CONFIG_KEYS exclusion list is needed.
 */
export function configToRecord (config: Config, explicitlySetKeys: Set<string>): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  // Add typed settings (only explicitly set ones if tracking is available)
  for (const kebabKey of Object.keys(types)) {
    const camelKey = camelcase(kebabKey, { locale: 'en-US' })
    if (!explicitlySetKeys.has(camelKey)) continue
    const value = (config as unknown as Record<string, unknown>)[camelKey]
    if (value !== undefined) {
      result[camelKey] = value
    }
  }
  // Add non-types config properties (e.g., packageExtensions, overrides)
  for (const [key, value] of Object.entries(config)) {
    if (value === undefined || NON_SETTING_CONFIG_KEYS.has(key)) continue
    if (!(key in result) && explicitlySetKeys.has(key)) {
      result[key] = value
    }
  }
  // Add auth/registry keys (scoped keys, auth tokens) — keep original casing
  for (const [key, value] of Object.entries(config.authConfig)) {
    if (!(key in result)) {
      result[key] = value
    }
  }
  // Always include user-agent for debugging connectivity issues
  if (config.userAgent) {
    result.userAgent = config.userAgent
  }
  return censorProtectedSettings(sortDirectKeys(result))
}
