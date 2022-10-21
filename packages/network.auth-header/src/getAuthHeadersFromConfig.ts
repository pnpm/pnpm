import { PnpmError } from '@pnpm/error'
import { spawnSync } from 'child_process'
import fs from 'fs'
import path from 'path'
import nerfDart from 'nerf-dart'

export function getAuthHeadersFromConfig (
  { allSettings, userSettings }: {
    allSettings: Record<string, string>
    userSettings: Record<string, string>
  }
) {
  const authHeaderValueByURI = {}
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
        const password = Buffer.from(allSettings[`${uri}:_password`], 'base64').toString('utf8')
        authHeaderValueByURI[uri] = `Basic ${Buffer.from(`${value}:${password}`).toString('base64')}`
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
    authHeaderValueByURI[registry] = `Basic ${Buffer.from(`${allSettings['username']}:${allSettings['_password']}`).toString('base64')}`
  }
  return authHeaderValueByURI
}

function splitKey (key: string) {
  const index = key.lastIndexOf(':')
  if (index === -1) {
    return [key, '']
  }
  return [key.slice(0, index), key.slice(index + 1)]
}

function loadToken (helperPath: string, settingName: string) {
  if (!path.isAbsolute(helperPath) || !fs.existsSync(helperPath)) {
    throw new PnpmError('BAD_TOKEN_HELPER_PATH', `${settingName} must be an absolute path, without arguments`)
  }

  const spawnResult = spawnSync(helperPath, { shell: true })

  if (spawnResult.status !== 0) {
    throw new PnpmError('TOKEN_HELPER_ERROR_STATUS', `Error running "${helperPath}" as a token helper, configured as ${settingName}. Exit code ${spawnResult.status?.toString() ?? ''}`)
  }
  return spawnResult.stdout.toString('utf8').trimEnd()
}
