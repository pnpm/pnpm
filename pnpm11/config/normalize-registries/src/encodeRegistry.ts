import { PnpmError } from '@pnpm/error'

export function encodeRegistry (registry: string): string {
  let urlObj: URL
  try {
    urlObj = new URL(registry)
  } catch (err: unknown) {
    throw new PnpmError(
      'INVALID_REGISTRY_URL',
      `Failed to parse registry URL "${registry}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err }
    )
  }
  if (!urlObj || !urlObj.host) {
    throw new PnpmError(
      'MISSING_REGISTRY_HOST',
      `Registry URL "${registry}" has no host`
    )
  }
  const host = urlObj.host.replaceAll(':', '+')
  const pathname = urlObj.pathname.replace(/^\/+|\/+$/g, '')
  if (!pathname) {
    return host
  }
  const encodedPath = pathname
    .replace(/%/g, '%25')
    .replace(/_/g, '%5F')
    .replace(/\+/g, '%2B')
    .replace(/:/g, '%3A')
    .replace(/\//g, '+')
  return `${host}_${encodedPath}`
}


