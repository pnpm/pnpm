import normalizeRegistryUrl from 'normalize-registry-url'

export function getScopeRegistries(rawConfig: Record<string, object>): Record<string, string> {
  const registries: Record<string, string> = {}

  for (const configKey of Object.keys(rawConfig)) {
    if (configKey.startsWith('@') && configKey.endsWith(':registry')) {
      registries[configKey.slice(0, configKey.indexOf(':'))] =
        normalizeRegistryUrl(rawConfig[configKey] as unknown as string)
    }
  }

  return registries
}
