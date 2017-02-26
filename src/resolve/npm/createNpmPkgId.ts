export default function createPkgId (
  registryHost: string,
  pkgName: string,
  pkgVersion: string
): string {
  const escapedRegistryHost = registryHost.replace(':', '+')
  return `${escapedRegistryHost}/${pkgName}/${pkgVersion}`
}
