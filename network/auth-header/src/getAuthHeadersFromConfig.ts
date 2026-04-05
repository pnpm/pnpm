import { spawnSync } from 'node:child_process'

import { PnpmError } from '@pnpm/error'
import type { Creds, TokenHelper } from '@pnpm/types'

export function getAuthHeadersFromCreds (
  credsByUri: Record<string, Creds>,
  defaultRegistry: string
): Record<string, string> {
  const authHeaderValueByURI: Record<string, string> = {}
  for (const [uri, parsedCreds] of Object.entries(credsByUri)) {
    if (uri === '') continue // default auth handled below
    const header = credsToHeader(parsedCreds)
    if (header) {
      authHeaderValueByURI[uri] = header
    }
  }
  const defaultAuth = credsByUri['']
  if (defaultAuth) {
    const header = credsToHeader(defaultAuth)
    if (header) {
      authHeaderValueByURI[defaultRegistry] = header
    }
  }
  return authHeaderValueByURI
}

function credsToHeader (parsedCreds: Creds): string | undefined {
  if (parsedCreds.tokenHelper) {
    return executeTokenHelper(parsedCreds.tokenHelper)
  }
  if (parsedCreds.authToken) {
    return `Bearer ${parsedCreds.authToken}`
  }
  if (parsedCreds.basicAuth) {
    return `Basic ${Buffer.from(`${parsedCreds.basicAuth.username}:${parsedCreds.basicAuth.password}`, 'utf8').toString('base64')}`
  }
  return undefined
}

function executeTokenHelper (tokenHelper: TokenHelper): string {
  const [cmd, ...args] = tokenHelper
  const spawnResult = spawnSync(cmd, args, { stdio: 'pipe' })

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
