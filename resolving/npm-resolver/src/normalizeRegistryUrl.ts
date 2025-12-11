/**
 * Remove default ports (80 for HTTP, 443 for HTTPS) to ensure consistency
 */
export function normalizeRegistryUrl (urlString: string): string {
  try {
    return new URL(urlString).toString()
  } catch {
    return urlString
  }
}
