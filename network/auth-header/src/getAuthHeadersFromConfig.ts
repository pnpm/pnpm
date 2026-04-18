import { spawnSync } from 'node:child_process'

import { PnpmError } from '@pnpm/error'
import type { Creds, RegistryConfig, TokenHelper } from '@pnpm/types'

export function getAuthHeadersFromCreds (
  configByUri: Record<string, RegistryConfig>,
  defaultRegistry: string
): Record<string, string> {
  const authHeaderValueByURI: Record<string, string> = {}
  for (const [uri, registryConfig] of Object.entries(configByUri)) {
    if (uri === '') continue // default auth handled below
    const header = credsToHeader(registryConfig.creds)
    if (header) {
      authHeaderValueByURI[uri] = header
    }
  }
  const defaultConfig = configByUri['']
  if (defaultConfig?.creds) {
    const header = credsToHeader(defaultConfig.creds)
    if (header) {
      authHeaderValueByURI[defaultRegistry] = header
    }
  }
  return authHeaderValueByURI
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
