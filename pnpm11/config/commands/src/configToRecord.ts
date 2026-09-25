import { pickRegistryContext, toResolvedRegistryDeclarations } from '@pnpm/config.normalize-registries'
import { type Config, toAuditSettings, toUpdateSettings, types } from '@pnpm/config.reader'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import camelcase from 'camelcase'

import { censorProtectedSettings } from './protectedSettings.js'

// Config fields the record does not copy verbatim: internal objects, the
// lookups and spellings the `registries` / `update` / `audit` settings are
// split into (shown re-joined under the documented names instead), and
// `catalogs`, which is re-added below as the resolved catalog set.
const NON_SETTING_CONFIG_KEYS = new Set([
  'authConfig', 'configByUri',
  'registriesByScope', 'registriesByPrefix', 'registryOptionsByUrl',
  'updateConfig', 'auditConfig', 'auditLevel',
  'catalogs',
])

/**
 * Convert a Config object to a camelCase record for display.
 * Only includes explicitly set values (from CLI, env vars, or workspace yaml),
 * not default values. Auth/registry keys from authConfig are always included,
 * and so is `registries` — the registries the CLI resolves from, merged
 * across every source.
 *
 * Accepts a clean Config object (without ConfigContext fields mixed in),
 * so no INTERNAL_CONFIG_KEYS exclusion list is needed.
 */
export function configToRecord (config: Config, explicitlySetKeys: Set<string>): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  // Add typed settings (only explicitly set ones if tracking is available)
  for (const kebabKey of Object.keys(types)) {
    const camelKey = camelcase(kebabKey, { locale: 'en-US' })
    if (!explicitlySetKeys.has(camelKey) || NON_SETTING_CONFIG_KEYS.has(camelKey)) continue
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
  // The `registry` / `@scope:registry` rows show the merged routes — the
  // values `config get` answers — so a raw `.npmrc` row cannot contradict
  // the resolved `registries` view.
  for (const [scope, url] of Object.entries(config.registriesByScope ?? {})) {
    result[scope === 'default' ? 'registry' : `${scope}:registry`] = url
  }
  // Always include user-agent for debugging connectivity issues
  if (config.userAgent) {
    result.userAgent = config.userAgent
  }
  result.registries = toResolvedRegistryDeclarations(pickRegistryContext(config))
  const update = toUpdateSettings(config.updateConfig)
  if (update != null) {
    result.update = update
  }
  const audit = toAuditSettings(config)
  if (audit != null) {
    result.audit = audit
  }
  // `catalogs` shows the complete resolved catalog set — the singular
  // `catalog` block is its `default` entry — whichever spelling declared it.
  if (config.catalogs != null && Object.values(config.catalogs).some((catalog) => catalog != null)) {
    result.catalogs = config.catalogs
  }
  return censorProtectedSettings(sortDirectKeys(result))
}
