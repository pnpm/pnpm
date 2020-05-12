import encodeRegistry = require('encode-registry')

export default function createPkgId (
  registry: string,
  pkgName: string,
  pkgVersion: string
): string {
  const escapedRegistryHost = encodeRegistry(registry)
  return `${escapedRegistryHost}/${pkgName}/${pkgVersion}`
}
