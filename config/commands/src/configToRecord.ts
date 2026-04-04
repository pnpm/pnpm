import { type Config, types } from '@pnpm/config.reader'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import camelcase from 'camelcase'

import { censorProtectedSettings } from './protectedSettings.js'

const INTERNAL_CONFIG_KEYS = new Set([
  'authConfig', 'authInfos', 'rawLocalConfig', 'cliOptions',
  'explicitlySetKeys',
  'hooks', 'finders', 'allProjects', 'selectedProjectsGraph',
  'packageManager', 'wantedPackageManager', 'rootProjectManifest',
  'storeController', 'rootProjectManifestDir', 'sslConfigs',
])

/**
 * Convert a Config object to a camelCase record for display.
 * Only includes explicitly set values (from CLI, env vars, or workspace yaml),
 * not default values. Auth/registry keys from authConfig are always included.
 */
export function configToRecord (config: Config): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  const explicit = config.explicitlySetKeys
  // Add typed settings (only explicitly set ones if tracking is available)
  for (const kebabKey of Object.keys(types)) {
    const camelKey = camelcase(kebabKey, { locale: 'en-US' })
    if (explicit && !explicit.has(camelKey)) continue
    const value = (config as unknown as Record<string, unknown>)[camelKey]
    if (value !== undefined) {
      result[camelKey] = value
    }
  }
  // Add non-types config properties (e.g., packageExtensions, overrides)
  for (const [key, value] of Object.entries(config)) {
    if (value === undefined || INTERNAL_CONFIG_KEYS.has(key)) continue
    if (!(key in result) && (!explicit || explicit.has(key))) {
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
