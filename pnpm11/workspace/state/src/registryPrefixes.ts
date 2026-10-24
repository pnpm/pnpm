import { pickDeclaredRegistryPrefix } from '@pnpm/config.pick-registry-for-package'
import type { RegistryContext } from '@pnpm/types'

/**
 * The `registryPrefixes` a workspace state records, keyed in sorted order:
 * `undefined` when no prefix is declared. See
 * `WorkspaceStateSettings.registryPrefixes`.
 */
export function recordedRegistryPrefixes (
  { registriesByPrefix, registriesByScope }: Partial<Pick<RegistryContext, 'registriesByPrefix' | 'registriesByScope'>>
): Record<string, string> | undefined {
  if (registriesByPrefix == null || Object.keys(registriesByPrefix).length === 0) return undefined
  const recorded: Record<string, string> = { ...registriesByPrefix }
  for (const [route, registry] of Object.entries(registriesByScope ?? {})) {
    const prefix = pickDeclaredRegistryPrefix(registriesByPrefix, registry)
    if (prefix != null) recorded[`route:${route}`] = prefix
  }
  return Object.fromEntries(Object.keys(recorded).sort().map((key) => [key, recorded[key]]))
}
