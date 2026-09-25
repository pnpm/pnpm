/**
 * Maps a registry URL to the key that its settings (`_authToken`, `certfile`,
 * and so on) are stored under in `.npmrc`. For instance,
 * `https://registry.npmjs.org/some-pkg` maps to `//registry.npmjs.org/`.
 *
 * npm calls this key a "nerf dart" and derives it the same way:
 * https://github.com/npm/cli/blob/latest/workspaces/config/lib/nerf-dart.js
 */
export function nerfDart (url: string): string {
  const parsed = new URL(url)
  const from = `${parsed.protocol}//${parsed.host}${parsed.pathname}`
  const rel = new URL('.', from)
  return `//${rel.host}${rel.pathname}`
}
