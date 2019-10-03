export default function getScopeRegistries (rawConfig: Object) {
  const registries = {}
  for (const configKey of Object.keys(rawConfig)) {
    if (configKey[0] === '@' && configKey.endsWith(':registry')) {
      registries[configKey.substr(0, configKey.indexOf(':'))] = normalizeRegistry(rawConfig[configKey])
    }
  }
  return registries
}

export function normalizeRegistry (registry: string) {
  return registry.endsWith('/') ? registry : `${registry}/`
}
