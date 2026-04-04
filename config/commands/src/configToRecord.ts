import { type Config, types } from '@pnpm/config.reader'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import camelcase from 'camelcase'

import { censorProtectedSettings } from './protectedSettings.js'

const INTERNAL_CONFIG_KEYS = new Set([
  'authConfig', 'rawLocalConfig', 'cliOptions',
  'hooks', 'finders', 'allProjects', 'selectedProjectsGraph',
  'packageManager', 'wantedPackageManager', 'rootProjectManifest',
  'storeController', 'rootProjectManifestDir',
])

/**
 * Convert a Config object to a camelCase record for display.
 * Merges typed settings with auth/registry keys from authConfig.
 */
export function configToRecord (config: Config): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  // Add typed settings using their camelCase names
  for (const kebabKey of Object.keys(types)) {
    const camelKey = camelcase(kebabKey, { locale: 'en-US' })
    const value = (config as unknown as Record<string, unknown>)[camelKey]
    if (value !== undefined) {
      result[camelKey] = value
    }
  }
  // Add non-types config properties (e.g., packageExtensions, overrides)
  for (const [key, value] of Object.entries(config)) {
    if (value === undefined || INTERNAL_CONFIG_KEYS.has(key)) continue
    if (!(key in result)) {
      result[key] = value
    }
  }
  // Add auth/registry keys (scoped keys, auth tokens) — keep original casing
  for (const [key, value] of Object.entries(config.authConfig)) {
    if (!(key in result)) {
      result[key] = value
    }
  }
  return censorProtectedSettings(sortDirectKeys(result))
}
