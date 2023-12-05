import nerfDart from 'nerf-dart'
import { getAuthHeadersFromConfig } from './getAuthHeadersFromConfig'
import { removePort } from './helpers/removePort'

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
  if (Object.keys(authHeaders).length === 0) return basicAuth
  return getAuthHeaderByURI.bind(null, authHeaders, getMaxParts(Object.keys(authHeaders)))
}

function getMaxParts (uris: string[]) {
  return uris.reduce((max, uri) => {
    const parts = uri.split('/').length
    return parts > max ? parts : max
  }, 0)
}

function getAuthHeaderByURI (authHeaders: Record<string, string>, maxParts: number, uri: string): string | undefined {
  if (!uri.endsWith('/')) {
    uri += '/'
  }
  const basic = basicAuth(uri)
  if (basic) return basic
  const nerfed = nerfDart(uri)
  const parts = nerfed.split('/')
  for (let i = Math.min(parts.length, maxParts) - 1; i >= 3; i--) {
    const key = `${parts.slice(0, i).join('/')}/`
    if (authHeaders[key]) return authHeaders[key]
  }
  const urlWithoutPort = removePort(uri)
  if (urlWithoutPort !== uri) {
    return getAuthHeaderByURI(authHeaders, maxParts, urlWithoutPort)
  }
  return undefined
}

function basicAuth (uri: string): string | undefined {
  const { username = '', password = '' } = new URL(uri)
  if (!username && !password) return undefined
  const auth64 = btoa(username + ':' + password)
  return `Basic ${auth64}`
}
