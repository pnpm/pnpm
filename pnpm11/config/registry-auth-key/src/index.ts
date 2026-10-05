import { isIP } from 'node:net'

/**
 * Maps a registry URL to the key that its settings (`_authToken`, `certfile`,
 * and so on) are stored under in `.npmrc`. For instance,
 * `https://registry.npmjs.org/some-pkg` maps to `//registry.npmjs.org/`.
 *
 * npm calls this key a "nerf dart" and derives it the same way:
 * https://github.com/npm/cli/blob/0c3b82a9a6/workspaces/config/lib/nerf-dart.js
 */
export function nerfDart (url: string): string {
  const parsed = new URL(url)
  const from = `${parsed.protocol}//${parsed.host}${parsed.pathname}`
  const rel = new URL('.', from)
  return `//${rel.host}${rel.pathname}`
}

export function isLoopbackHost (hostname: string): boolean {
  const host = hostname.startsWith('[') && hostname.endsWith(']') ? hostname.slice(1, -1) : hostname
  if (host.toLowerCase() === 'localhost') return true
  const ipVersion = isIP(host)
  if (ipVersion === 4) {
    return host.startsWith('127.')
  }
  if (ipVersion === 6) {
    return host === '::1'
  }
  return false
}

export function isUrlSecureForCredentials (url: string | URL): boolean {
  try {
    const parsed = typeof url === 'string' ? new URL(url) : url
    return parsed.protocol === 'https:' || (parsed.protocol === 'http:' && isLoopbackHost(parsed.hostname))
  } catch {
    return false
  }
}

