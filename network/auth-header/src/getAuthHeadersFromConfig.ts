import { spawnSync } from 'node:child_process'

import { PnpmError } from '@pnpm/error'
import type { Creds, TokenHelper } from '@pnpm/types'

export function getAuthHeadersFromAuthInfos (
  authInfos: Record<string, Creds>,
  defaultRegistry: string
): Record<string, string> {
  const authHeaderValueByURI: Record<string, string> = {}
  for (const [key, authInfo] of Object.entries(authInfos)) {
    if (key === '') continue // default auth handled below
    const header = authInfoToHeader(authInfo)
    if (header) {
      authHeaderValueByURI[key] = header
    }
  }
  const defaultAuth = authInfos['']
  if (defaultAuth) {
    const header = authInfoToHeader(defaultAuth)
    if (header) {
      authHeaderValueByURI[defaultRegistry] = header
    }
  }
  return authHeaderValueByURI
}

function authInfoToHeader (authInfo: Creds): string | undefined {
  if (authInfo.tokenHelper) {
    return executeTokenHelper(authInfo.tokenHelper)
  }
  if (authInfo.authToken) {
    return `Bearer ${authInfo.authToken}`
  }
  if (authInfo.authUserPass) {
    return `Basic ${btoa(`${authInfo.authUserPass.username}:${authInfo.authUserPass.password}`)}`
  }
  return undefined
}

function executeTokenHelper (tokenHelper: TokenHelper): string {
  const [cmd, ...args] = tokenHelper
  const spawnResult = spawnSync(cmd, args, { stdio: 'pipe', shell: true })

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
