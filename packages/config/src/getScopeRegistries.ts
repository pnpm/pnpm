import normalizeRegistryUrl from 'normalize-registry-url'

export function getScopeRegistries (rawConfig: Object) {
  const registries = {}
  for (const configKey of Object.keys(rawConfig)) {
    if (configKey[0] === '@' && configKey.endsWith(':registry')) {
      registries[configKey.slice(0, configKey.indexOf(':'))] = normalizeRegistryUrl(rawConfig[configKey])
    }
  }
  return registries
}
