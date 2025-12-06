export function normalizeRegistryUrl (urlString: string): string {
  // Remove default ports (80 for HTTP, 443 for HTTPS) to ensure consistency
  try {
    const urlObj = new URL(urlString)
    if ((urlObj.protocol === 'https:' && urlObj.port === '443') ||
        (urlObj.protocol === 'http:' && urlObj.port === '80')) {
      urlObj.port = ''
    }
    return urlObj.toString()
  } catch {
    return urlString
  }
}
