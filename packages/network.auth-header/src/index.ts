import nerfDart from 'nerf-dart'
import { getAuthHeadersFromConfig } from './getAuthHeadersFromConfig'

export function createGetAuthHeaderByURI (
  opts: {
    allSettings: Record<string, string>
    userSettings?: Record<string, string>
  }
) {
  const authHeaders = getAuthHeadersFromConfig({
    allSettings: opts.allSettings,
    userSettings: opts.userSettings ?? {},
  })
  if (Object.keys(authHeaders).length === 0) return () => undefined
  return getAuthHeaderByURI.bind(null, authHeaders, getMaxParts(Object.keys(authHeaders)))
}

function getMaxParts (uris: string[]) {
  return uris.reduce((max, uri) => {
    const parts = uri.split('/').length
    return parts > max ? parts : max
  }, 0)
}

function getAuthHeaderByURI (authHeaders: Record<string, string>, maxParts: number, uri: string) {
  const nerfed = nerfDart(uri)
  const parts = nerfed.split('/')
  for (let i = Math.min(parts.length, maxParts) - 1; i >= 3; i--) {
    const key = `${parts.slice(0, i).join('/')}/` // eslint-disable-line
    if (authHeaders[key]) return authHeaders[key]
  }
  return undefined
}
