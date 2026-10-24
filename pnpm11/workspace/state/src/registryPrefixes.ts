/**
 * The `registryPrefixes` a workspace state records for the declared prefixes,
 * keyed in sorted order: `undefined` when none is declared. See
 * `WorkspaceStateSettings.registryPrefixes`.
 */
export function recordedRegistryPrefixes (registriesByPrefix: Record<string, string> | undefined): Record<string, string> | undefined {
  if (registriesByPrefix == null) return undefined
  const prefixes = Object.keys(registriesByPrefix).sort()
  if (prefixes.length === 0) return undefined
  return Object.fromEntries(prefixes.map((prefix) => [prefix, registriesByPrefix[prefix]]))
}
