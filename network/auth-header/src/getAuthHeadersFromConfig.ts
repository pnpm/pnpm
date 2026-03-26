import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { nerfDart } from '@pnpm/config.nerf-dart'
import { PnpmError } from '@pnpm/error'

export function getAuthHeadersFromConfig (
  { allSettings, userSettings }: {
    allSettings: Record<string, string>
    userSettings: Record<string, string>
  }
): Record<string, string> {
  const authHeaderValueByURI: Record<string, string> = {}
  for (const [key, value] of Object.entries(allSettings)) {
    const [uri, authType] = splitKey(key)
    switch (authType) {
      case '_authToken': {
        authHeaderValueByURI[uri] = `Bearer ${value}`
        continue
      }
      case '_auth': {
        authHeaderValueByURI[uri] = `Basic ${value}`
        continue
      }
      case 'username': {
        if (`${uri}:_password` in allSettings) {
          authHeaderValueByURI[uri] = basicAuth(value, allSettings[`${uri}:_password`])
        }
      }
    }
  }
  for (const [key, value] of Object.entries(userSettings)) {
    const [uri, authType] = splitKey(key)
    if (authType === 'tokenHelper') {
      authHeaderValueByURI[uri] = loadToken(value, key)
    }
  }
  const registry = allSettings['registry'] ? nerfDart(allSettings['registry']) : '//registry.npmjs.org/'
  if (userSettings['tokenHelper']) {
    authHeaderValueByURI[registry] = loadToken(userSettings['tokenHelper'], 'tokenHelper')
  } else if (allSettings['_authToken']) {
    authHeaderValueByURI[registry] = `Bearer ${allSettings['_authToken']}`
  } else if (allSettings['_auth']) {
    authHeaderValueByURI[registry] = `Basic ${allSettings['_auth']}`
  } else if (allSettings['_password'] && allSettings['username']) {
    authHeaderValueByURI[registry] = basicAuth(allSettings['username'], allSettings['_password'])
  }
  return authHeaderValueByURI
}

function basicAuth (username: string, encodedPassword: string): string {
  const password = Buffer.from(encodedPassword, 'base64').toString('utf8')
  return `Basic ${Buffer.from(`${username}:${password}`).toString('base64')}`
}

function splitKey (key: string): string[] {
  const index = key.lastIndexOf(':')
  if (index === -1) {
    return [key, '']
  }
  return [key.slice(0, index), key.slice(index + 1)]
}

export function loadToken (helperPath: string, settingName: string): string {
  if (!path.isAbsolute(helperPath) || !fs.existsSync(helperPath)) {
    throw new PnpmError('BAD_TOKEN_HELPER_PATH', `${settingName} must be an absolute path, without arguments`)
  }

  const spawnResult = spawnSync(helperPath, { shell: true })

  if (spawnResult.status !== 0) {
    throw new PnpmError('TOKEN_HELPER_ERROR_STATUS', `Error running "${helperPath}" as a token helper, configured as ${settingName}. Exit code ${spawnResult.status?.toString() ?? ''}`)
  }
  const token = spawnResult.stdout.toString('utf8').trimEnd()
  if (!token) {
    throw new PnpmError('TOKEN_HELPER_EMPTY_TOKEN', `Token helper "${helperPath}", configured as ${settingName}, returned an empty token`)
  }
  // If the token already contains an auth scheme (e.g. "Bearer ...", "Basic ..."),
  // return it as-is.
  if (/^[A-Z]+ /i.test(token)) {
    return token
  }
  return `Bearer ${token}`
}
