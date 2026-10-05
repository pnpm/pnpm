import { isUrlSecureForCredentials, nerfDart } from '@pnpm/config.registry-auth-key'
import type { RegistryConfig } from '@pnpm/types'

import { type AuthHeaders, type AuthHeadersByScope, getAuthHeadersByScope, getAuthHeadersFromCreds } from './getAuthHeadersFromConfig.js'
import { removePort } from './helpers/removePort.js'

// Re-exported so callers can build the same URL/scoped credential lookup
// without re-implementing `credsToHeader`.
export { type AuthHeaders, type AuthHeadersByScope, getAuthHeadersByScope, getAuthHeadersFromCreds, isUrlSecureForCredentials }

interface GetAuthHeaderOptions {
  pkgName?: string
}

interface AuthHeaderLookup {
  insecureUris: Set<string>
  maxParts: number
  scopedAuthHeaderValueByScope: Record<string, ScopedAuthHeaderLookup>
}

interface ScopedAuthHeaderLookup {
  authHeaderValueByURI: Record<string, string>
  maxParts: number
}

export function createGetAuthHeaderByURI (
  configByUri: Record<string, RegistryConfig>,
  opts?: { allowedInsecureUris?: string[] | Set<string> }
): (uri: string, opts?: GetAuthHeaderOptions) => string | undefined {
  const authHeaders = getAuthHeadersFromCreds(configByUri)
  const registryURIs = Object.keys(authHeaders.authHeaderValueByURI)
  const scopedAuthHeaderValueByScope = getScopedAuthHeaderValueByScope(authHeaders.scopedAuthHeaderValueByURI)
  if (registryURIs.length === 0 && Object.keys(scopedAuthHeaderValueByScope).length === 0) {
    return fallbackAuth
  }
  return getAuthHeaderByURI.bind(null, authHeaders, {
    insecureUris: collectInsecureUris(configByUri, opts?.allowedInsecureUris),
    maxParts: getMaxParts(registryURIs),
    scopedAuthHeaderValueByScope,
  })
}

function fallbackAuth (uri: string): string | undefined {
  try {
    return basicAuth(new URL(uri))
  } catch {
    return undefined
  }
}

function collectInsecureUris (
  configByUri: Record<string, RegistryConfig>,
  allowed?: string[] | Set<string>
): Set<string> {
  const insecureUris = new Set<string>()
  for (const uri of Object.keys(configByUri)) {
    if (uri.startsWith('http://') && !isUrlSecureForCredentials(uri)) {
      addInsecureUri(insecureUris, uri)
    }
  }
  if (allowed) {
    for (const uri of allowed) {
      addInsecureUri(insecureUris, uri)
    }
  }
  return insecureUris
}

function addInsecureUri (insecureUris: Set<string>, uri: string): void {
  if (uri.startsWith('http://') || uri.startsWith('https://')) {
    try {
      insecureUris.add(nerfDart(uri))
    } catch {}
  }
  insecureUris.add(uri.endsWith('/') ? uri : `${uri}/`)
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
  let parsedUri: URL
  try {
    parsedUri = new URL(uri)
  } catch {
    return undefined
  }
  const isSecure = isUrlSecureForCredentials(parsedUri)
  const basic = basicAuth(parsedUri)
  if (basic) return basic
  const scope = getScope(opts?.pkgName)
  const scopedAuthHeaderLookup = scope ? lookup.scopedAuthHeaderValueByScope[scope] : undefined
  if (scopedAuthHeaderLookup) {
    const scopedAuth = getAuthHeaderByNerfedURI(scopedAuthHeaderLookup.authHeaderValueByURI, scopedAuthHeaderLookup.maxParts, uri, isSecure, lookup.insecureUris)
    if (scopedAuth) return scopedAuth
  }
  return getAuthHeaderByNerfedURI(authHeaders.authHeaderValueByURI, lookup.maxParts, uri, isSecure, lookup.insecureUris)
}

function getAuthHeaderByNerfedURI (
  authHeaders: Record<string, string>,
  maxParts: number,
  uri: string,
  isSecure: boolean,
  insecureUris: Set<string>
): string | undefined {
  const parsedUri = new URL(uri)
  const nerfed = nerfDart(uri)
  const parts = nerfed.split('/')
  for (let partCount = Math.min(parts.length, maxParts) - 1; partCount >= 3; partCount--) {
    const key = `${parts.slice(0, partCount).join('/')}/`
    if (authHeaders[key]) {
      if (isSecure || insecureUris.has(key)) {
        return authHeaders[key]
      }
      return undefined
    }
  }
  const urlWithoutPort = removePort(parsedUri)
  if (urlWithoutPort !== uri) {
    return getAuthHeaderByNerfedURI(authHeaders, maxParts, urlWithoutPort, isSecure, insecureUris)
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
  if (!isUrlSecureForCredentials(uri)) return undefined
  const auth64 = btoa(`${uri.username}:${uri.password}`)
  return `Basic ${auth64}`
}

