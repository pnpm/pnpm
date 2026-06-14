import { nerfDart } from '@pnpm/config.nerf-dart'
import type { RegistryConfig } from '@pnpm/types'

import { type AuthHeaders, type AuthHeadersByScope, getAuthHeadersByScope, getAuthHeadersFromCreds } from './getAuthHeadersFromConfig.js'
import { removePort } from './helpers/removePort.js'

// Re-exported so callers can build the same URL/scoped credential lookup
// without re-implementing `credsToHeader`.
export { type AuthHeaders, type AuthHeadersByScope, getAuthHeadersByScope, getAuthHeadersFromCreds }

interface GetAuthHeaderOptions {
  pkgName?: string
}

interface AuthHeaderLookup {
  maxParts: number
  scopedAuthHeaderValueByScope: Record<string, ScopedAuthHeaderLookup>
}

interface ScopedAuthHeaderLookup {
  authHeaderValueByURI: Record<string, string>
  maxParts: number
}

export function createGetAuthHeaderByURI (
  configByUri: Record<string, RegistryConfig>
): (uri: string, opts?: GetAuthHeaderOptions) => string | undefined {
  const authHeaders = getAuthHeadersFromCreds(configByUri)
  const registryURIs = Object.keys(authHeaders.authHeaderValueByURI)
  const scopedAuthHeaderValueByScope = getScopedAuthHeaderValueByScope(authHeaders.scopedAuthHeaderValueByURI)
  if (registryURIs.length === 0 && Object.keys(scopedAuthHeaderValueByScope).length === 0) return (uri: string) => basicAuth(new URL(uri))
  return getAuthHeaderByURI.bind(null, authHeaders, {
    maxParts: getMaxParts(registryURIs),
    scopedAuthHeaderValueByScope,
  })
}

function getMaxParts (uris: string[]): number {
  return uris.reduce((max, uri) => {
    const parts = uri.split('/').length
    return parts > max ? parts : max
  }, 0)
}

function getScopedAuthHeaderValueByScope (
  authHeaders: Record<string, Record<string, string>>
): Record<string, ScopedAuthHeaderLookup> {
  const result: Record<string, ScopedAuthHeaderLookup> = {}
  for (const [uri, scopedAuthHeaders] of Object.entries(authHeaders)) {
    const parts = uri.split('/').length
    for (const [scope, authHeader] of Object.entries(scopedAuthHeaders)) {
      const scopedAuthHeaderLookup = result[scope] ??= {
        authHeaderValueByURI: {},
        maxParts: 0,
      }
      scopedAuthHeaderLookup.authHeaderValueByURI[uri] = authHeader
      if (parts > scopedAuthHeaderLookup.maxParts) {
        scopedAuthHeaderLookup.maxParts = parts
      }
    }
  }
  return result
}

function getAuthHeaderByURI (
  authHeaders: AuthHeaders,
  lookup: AuthHeaderLookup,
  uri: string,
  opts?: GetAuthHeaderOptions
): string | undefined {
  if (!uri.endsWith('/')) {
    uri += '/'
  }
  const parsedUri = new URL(uri)
  const basic = basicAuth(parsedUri)
  if (basic) return basic
  const scope = getScope(opts?.pkgName)
  const scopedAuthHeaderLookup = scope ? lookup.scopedAuthHeaderValueByScope[scope] : undefined
  if (scopedAuthHeaderLookup) {
    const scopedAuth = getAuthHeaderByNerfedURI(scopedAuthHeaderLookup.authHeaderValueByURI, scopedAuthHeaderLookup.maxParts, uri)
    if (scopedAuth) return scopedAuth
  }
  return getAuthHeaderByNerfedURI(authHeaders.authHeaderValueByURI, lookup.maxParts, uri)
}

function getAuthHeaderByNerfedURI (authHeaders: Record<string, string>, maxParts: number, uri: string): string | undefined {
  const parsedUri = new URL(uri)
  const nerfed = nerfDart(uri)
  const parts = nerfed.split('/')
  for (let i = Math.min(parts.length, maxParts) - 1; i >= 3; i--) {
    const key = `${parts.slice(0, i).join('/')}/`
    if (authHeaders[key]) return authHeaders[key]
  }
  const urlWithoutPort = removePort(parsedUri)
  if (urlWithoutPort !== uri) {
    return getAuthHeaderByNerfedURI(authHeaders, maxParts, urlWithoutPort)
  }
  return undefined
}

function getScope (pkgName: string | undefined): string | undefined {
  if (!pkgName?.startsWith('@')) return undefined
  const index = pkgName.indexOf('/')
  if (index <= 1) return undefined
  return pkgName.slice(0, index)
}

function basicAuth (uri: URL): string | undefined {
  if (!uri.username && !uri.password) return undefined
  const auth64 = btoa(`${uri.username}:${uri.password}`)
  return `Basic ${auth64}`
}
