import encodeRegistry from 'encode-registry'

export function createPkgId (
  registry: string,
  pkgName: string,
  pkgVersion: string
): string {
  const escapedRegistryHost = encodeRegistry(registry)
  return `${escapedRegistryHost}/${pkgName}/${pkgVersion}`
}
