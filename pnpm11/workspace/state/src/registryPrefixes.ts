import type { RegistriesByScope, RegistryContext } from '@pnpm/types'

/**
 * The `registryPrefixes` a workspace state records, keyed in sorted order:
 * `undefined` when no prefix is declared. See
 * `WorkspaceStateSettings.registryPrefixes`.
 */
export function recordedRegistryPrefixes (
  { registriesByPrefix }: Partial<Pick<RegistryContext, 'registriesByPrefix'>>
): Record<string, string> | undefined {
  if (registriesByPrefix == null || Object.keys(registriesByPrefix).length === 0) return undefined
  return sortedEntries(registriesByPrefix)
}

/**
 * The scope routes an install resolves plain keys through, keyed in sorted
 * order, as `WorkspaceStateSettings.registryRoutes` records them.
 */
export function recordedRegistryRoutes (registriesByScope: RegistriesByScope): Record<string, string> {
  const { default: _default, ...scopes } = registriesByScope
  return sortedEntries(scopes)
}

function sortedEntries (record: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.keys(record).sort().map((key) => [key, record[key]]))
}
