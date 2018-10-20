export default function getScopeRegistries (rawNpmConfig: Object) {
  const registries = {}
  for (const configKey of Object.keys(rawNpmConfig)) {
    if (configKey[0] === '@' && configKey.endsWith(':registry')) {
      registries[configKey.substr(0, configKey.indexOf(':'))] = rawNpmConfig[configKey]
    }
  }
  return registries
}
