import { URL } from 'url'

// https://github.com/npm/cli/blob/latest/workspaces/config/lib/nerf-dart.js
export function nerfDart (url: string): string {
  const parsed = new URL(url)
  const from = `${parsed.protocol}//${parsed.host}${parsed.pathname}`
  const rel = new URL('.', from)
  const res = `//${rel.host}${rel.pathname}`
  return res
}
