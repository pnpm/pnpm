import { isIP } from 'node:net'

import { PnpmError } from '@pnpm/error'

export function resolveServerUrl (serverUrl: string | undefined): string {
  const parsed = URL.parse(serverUrl || process.env.GITHUB_SERVER_URL || 'https://github.com')
  const loopback = parsed != null && (parsed.hostname === 'localhost' || parsed.hostname === '[::1]' || (isIP(parsed.hostname) === 4 && parsed.hostname.startsWith('127.')))
  if (parsed == null || (parsed.protocol !== 'https:' && !(parsed.protocol === 'http:' && loopback))) {
    throw new PnpmError('GITHUB_ACTIONS_SERVER_PROTOCOL', 'The GitHub Actions server URL must use HTTPS, except for HTTP on loopback hosts')
  }
  let url = parsed.href
  while (url.endsWith('/')) url = url.slice(0, -1)
  return url
}
