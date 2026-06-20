import { spawnSync } from 'node:child_process'

import { PnpmError } from '@pnpm/error'
import { type Creds, DEFAULT_REGISTRY_SCOPE, type RegistryConfig, type TokenHelper } from '@pnpm/types'

export interface AuthHeaders {
  authHeaderValueByURI: Record<string, string>
  scopedAuthHeaderValueByURI: Record<string, Record<string, string>>
}

export type AuthHeadersByScope = Record<string, Record<string, string>>

export function getAuthHeadersFromCreds (
  configByUri: Record<string, RegistryConfig>
): AuthHeaders {
  const authHeaders: AuthHeaders = {
    authHeaderValueByURI: {},
    scopedAuthHeaderValueByURI: {},
  }
  for (const [uri, registryConfig] of Object.entries(configByUri)) {
    const normalizedUri = normalizeAuthKey(uri)
    const header = credsToHeader(registryConfig[DEFAULT_REGISTRY_SCOPE])
    if (header) {
      authHeaders.authHeaderValueByURI[normalizedUri] = header
    }
    for (const scope of getRegistryScopes(registryConfig)) {
      if (scope === DEFAULT_REGISTRY_SCOPE) continue
      const scopedCreds = registryConfig[scope]
      const scopedHeader = credsToHeader(scopedCreds)
      if (scopedHeader) {
        authHeaders.scopedAuthHeaderValueByURI[normalizedUri] ??= {}
        authHeaders.scopedAuthHeaderValueByURI[normalizedUri][scope] = scopedHeader
      }
    }
  }
  return authHeaders
}

export function getAuthHeadersByScope (authHeaders: AuthHeaders): AuthHeadersByScope {
  const result: AuthHeadersByScope = {}
  for (const [registryURI, authHeader] of Object.entries(authHeaders.authHeaderValueByURI)) {
    result[registryURI] ??= {}
    result[registryURI][DEFAULT_REGISTRY_SCOPE] = authHeader
  }
  for (const [registryURI, scopedAuthHeaders] of Object.entries(authHeaders.scopedAuthHeaderValueByURI)) {
    result[registryURI] ??= {}
    for (const [scope, authHeader] of Object.entries(scopedAuthHeaders)) {
      result[registryURI][scope] = authHeader
    }
  }
  return result
}

function getRegistryScopes (registryConfig: RegistryConfig): Array<`@${string}`> {
  return Object.keys(registryConfig).filter((scope): scope is `@${string}` => scope.startsWith('@'))
}

function normalizeAuthKey (uri: string): string {
  if (!uri) return uri
  return uri.endsWith('/') ? uri : `${uri}/`
}

function credsToHeader (creds?: Creds): string | undefined {
  if (!creds) return undefined
  if (creds.tokenHelper) {
    return executeTokenHelper(creds.tokenHelper)
  }
  if (creds.authToken) {
    return `Bearer ${creds.authToken}`
  }
  if (creds.basicAuth) {
    return `Basic ${Buffer.from(`${creds.basicAuth.username}:${creds.basicAuth.password}`, 'utf8').toString('base64')}`
  }
  return undefined
}

function executeTokenHelper (tokenHelper: TokenHelper): string {
  const [cmd, ...args] = tokenHelper
  // On Windows, .bat/.cmd files require a shell to execute.
  const shell = process.platform === 'win32' && /\.(?:bat|cmd)$/i.test(cmd)
  const spawnResult = spawnSync(cmd, args, { stdio: 'pipe', shell })

  if (spawnResult.status !== 0) {
    throw new PnpmError('TOKEN_HELPER_ERROR_STATUS', `Error running "${cmd}" as a token helper. Exit code ${spawnResult.status?.toString() ?? ''}`)
  }
  const token = spawnResult.stdout.toString('utf8').trimEnd()
  if (!token) {
    throw new PnpmError('TOKEN_HELPER_EMPTY_TOKEN', `Token helper "${cmd}" returned an empty token`)
  }
  // If the token already contains an auth scheme (e.g. "Bearer ...", "Basic ..."),
  // return it as-is.
  if (/^[A-Z]+ /i.test(token)) {
    return token
  }
  return `Bearer ${token}`
}
