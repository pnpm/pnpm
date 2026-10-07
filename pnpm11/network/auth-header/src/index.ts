import { isUrlSecureForCredentials, nerfDart } from '@pnpm/config.registry-auth-key'
import { type Creds, DEFAULT_REGISTRY_SCOPE, type RegistryConfig } from '@pnpm/types'

import { type AuthHeaders, type AuthHeadersByScope, getAuthHeadersByScope, getAuthHeadersFromCreds } from './getAuthHeadersFromConfig.js'
import { removePort } from './helpers/removePort.js'

// Re-exported so callers can build the same URL/scoped credential lookup
// without re-implementing `credsToHeader`.
export { type AuthHeaders, type AuthHeadersByScope, getAuthHeadersByScope, getAuthHeadersFromCreds, isUrlSecureForCredentials }

interface GetAuthHeaderOptions {
  pkgName?: string
}

interface AuthHeaderLookup {
  authHeaderValueByURI: Record<string, string>
  httpAuthHeaderValueByURI: Record<string, string>
  httpsUris: Set<string>
  insecureUris: Set<string>
  maxParts: number
  httpMaxParts: number
  scopedAuthHeaderValueByScope: Record<string, ScopedAuthHeaderLookup>
}

interface ScopedAuthHeaderLookup {
  authHeaderValueByURI: Record<string, string>
  httpAuthHeaderValueByURI: Record<string, string>
  maxParts: number
  httpMaxParts: number
  insecureUris: Set<string>
}

export function createGetAuthHeaderByURI (
  configByUri: Record<string, RegistryConfig>,
  opts?: { allowedInsecureUris?: string[] | Set<string> }
): (uri: string, opts?: GetAuthHeaderOptions) => string | undefined {
  const authHeaders = getAuthHeadersFromCreds(configByUri)
  const registryURIs = Object.keys(authHeaders.authHeaderValueByURI)
  const httpRegistryURIs = Object.keys(authHeaders.httpAuthHeaderValueByURI)
  const scopedAuthHeaderValueByScope = getScopedAuthHeaderValueByScope(
    authHeaders.scopedAuthHeaderValueByURI,
    authHeaders.scopedHttpAuthHeaderValueByURI,
    configByUri,
    opts?.allowedInsecureUris
  )
  if (
    registryURIs.length === 0 &&
    httpRegistryURIs.length === 0 &&
    Object.keys(scopedAuthHeaderValueByScope).length === 0
  ) {
    return fallbackAuth
  }
  return getAuthHeaderByURI.bind(null, {
    authHeaderValueByURI: authHeaders.authHeaderValueByURI,
    httpAuthHeaderValueByURI: authHeaders.httpAuthHeaderValueByURI,
    httpsUris: authHeaders.httpsUris,
    insecureUris: collectInsecureUris(configByUri, DEFAULT_REGISTRY_SCOPE, opts?.allowedInsecureUris),
    maxParts: getMaxParts(registryURIs),
    httpMaxParts: getMaxParts(httpRegistryURIs),
    scopedAuthHeaderValueByScope,
  })
}

function fallbackAuth (uri: string): string | undefined {
  try {
    return basicAuth(new URL(uri))
  } catch {
    // Malformed URLs cannot have basic auth credentials and are ignored.
    return undefined
  }
}

function collectInsecureUris (
  configByUri: Record<string, RegistryConfig>,
  scope: string,
  allowed?: string[] | Set<string>
): Set<string> {
  const insecureUris = new Set<string>()
  for (const [uri, config] of Object.entries(configByUri)) {
    if (uri.startsWith('http://') && !isUrlSecureForCredentials(uri) && hasScopeCredentials(config, scope)) {
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

function hasScopeCredentials (registryConfig: RegistryConfig, scope: string): boolean {
  return hasCreds(registryConfig[scope as keyof RegistryConfig] as Creds | undefined)
}

function hasCreds (creds?: Creds): boolean {
  return creds != null && Boolean(creds.authToken || creds.basicAuth || creds.tokenHelper)
}

function addInsecureUri (insecureUris: Set<string>, uri: string): void {
  if (uri.startsWith('http://') || uri.startsWith('https://')) {
    try {
      insecureUris.add(nerfDart(uri))
    } catch {
      // Malformed URIs cannot produce a nerf dart and are skipped.
    }
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
  authHeaders: Record<string, Record<string, string>>,
  httpAuthHeaders: Record<string, Record<string, string>>,
  configByUri: Record<string, RegistryConfig>,
  allowedInsecureUris?: string[] | Set<string>
): Record<string, ScopedAuthHeaderLookup> {
  const result: Record<string, ScopedAuthHeaderLookup> = {}
  populateScopedLookup(authHeaders, result, 'authHeaderValueByURI', configByUri, allowedInsecureUris)
  populateScopedLookup(httpAuthHeaders, result, 'httpAuthHeaderValueByURI', configByUri, allowedInsecureUris)
  return result
}

function populateScopedLookup (
  scopedByUri: Record<string, Record<string, string>>,
  result: Record<string, ScopedAuthHeaderLookup>,
  field: 'authHeaderValueByURI' | 'httpAuthHeaderValueByURI',
  configByUri: Record<string, RegistryConfig>,
  allowedInsecureUris?: string[] | Set<string>
): void {
  const maxPartsField = field === 'httpAuthHeaderValueByURI' ? 'httpMaxParts' : 'maxParts'
  for (const [uri, scopedHeaders] of Object.entries(scopedByUri)) {
    const parts = uri.split('/').length
    for (const [scope, authHeader] of Object.entries(scopedHeaders)) {
      const scopedLookup = result[scope] ??= createScopedAuthHeaderLookup(
        configByUri,
        scope,
        allowedInsecureUris
      )
      scopedLookup[field][uri] = authHeader
      if (parts > scopedLookup[maxPartsField]) {
        scopedLookup[maxPartsField] = parts
      }
    }
  }
}

function createScopedAuthHeaderLookup (
  configByUri: Record<string, RegistryConfig>,
  scope: string,
  allowedInsecureUris?: string[] | Set<string>
): ScopedAuthHeaderLookup {
  return {
    authHeaderValueByURI: {},
    httpAuthHeaderValueByURI: {},
    maxParts: 0,
    httpMaxParts: 0,
    insecureUris: collectInsecureUris(configByUri, scope, allowedInsecureUris),
  }
}

function getAuthHeaderByURI (
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
    // Malformed request URLs cannot be matched against registry auth keys.
    return undefined
  }
  const isSecure = isUrlSecureForCredentials(parsedUri)
  const basic = basicAuth(parsedUri)
  if (basic) return basic
  const scope = getScope(opts?.pkgName)
  const scopedAuthHeaderLookup = scope ? lookup.scopedAuthHeaderValueByScope[scope] : undefined
  if (scopedAuthHeaderLookup) {
    const scopedAuth = getAuthHeaderByNerfedURI(
      scopedAuthHeaderLookup,
      lookup.httpsUris,
      uri,
      isSecure
    )
    if (scopedAuth) return scopedAuth
  }
  return getAuthHeaderByNerfedURI(
    {
      authHeaderValueByURI: lookup.authHeaderValueByURI,
      httpAuthHeaderValueByURI: lookup.httpAuthHeaderValueByURI,
      insecureUris: lookup.insecureUris,
      maxParts: lookup.maxParts,
      httpMaxParts: lookup.httpMaxParts,
    },
    lookup.httpsUris,
    uri,
    isSecure
  )
}

interface LookupTarget {
  authHeaderValueByURI: Record<string, string>
  httpAuthHeaderValueByURI: Record<string, string>
  maxParts: number
  httpMaxParts: number
  insecureUris: Set<string>
}

function getAuthHeaderByNerfedURI (
  target: LookupTarget,
  httpsUris: Set<string>,
  uri: string,
  isSecure: boolean
): string | undefined {
  const parsedUri = new URL(uri)
  const nerfed = nerfDart(uri)
  const parts = nerfed.split('/')
  const maxParts = Math.max(target.maxParts, target.httpMaxParts)
  const candidate = findMatchingCandidate(target, parts, maxParts, isSecure, httpsUris)
  if (candidate) return candidate

  if (isSecure) {
    const urlWithoutPort = removePort(parsedUri)
    if (urlWithoutPort !== uri) {
      return getAuthHeaderByNerfedURI(target, httpsUris, urlWithoutPort, isSecure)
    }
  }
  return undefined
}

function findMatchingCandidate (
  target: LookupTarget,
  parts: string[],
  maxParts: number,
  isSecure: boolean,
  httpsUris: Set<string>
): string | undefined {
  for (let partCount = Math.min(parts.length, maxParts) - 1; partCount >= 3; partCount--) {
    const key = `${parts.slice(0, partCount).join('/')}/`
    const candidate = pickCandidate(target, key, isSecure)
    if (candidate) {
      return isKeyAllowed(key, isSecure, target.insecureUris, httpsUris) ? candidate : undefined
    }
  }
  return undefined
}

function pickCandidate (
  target: LookupTarget,
  key: string,
  isSecure: boolean
): string | undefined {
  if (isSecure) {
    return target.authHeaderValueByURI[key] ?? target.httpAuthHeaderValueByURI[key]
  }
  return target.httpAuthHeaderValueByURI[key] ?? target.authHeaderValueByURI[key]
}

function isKeyAllowed (
  key: string,
  isSecure: boolean,
  insecureUris: Set<string>,
  httpsUris: Set<string>
): boolean {
  if (isSecure) return true
  if (httpsUris.has(key)) return false
  for (const insecureUri of insecureUris) {
    if (key === insecureUri || key.startsWith(insecureUri)) return true
  }
  return false
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

