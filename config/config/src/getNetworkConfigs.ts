import type { SslConfig } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'
import fs from 'fs'
import { type AuthInfo, type AuthInfoInput, parseAuthInfo } from './parseAuthInfo.js'

export interface NetworkConfigs {
  authInfos?: Record<string, AuthInfo> // TODO: remove optional from here, this means that tests would have to be updated.
  sslConfigs: Record<string, SslConfig>
  registries: Record<string, string>
}

export function getNetworkConfigs (rawConfig: Record<string, unknown>): NetworkConfigs {
  const authInfoInputs: Record<string, AuthInfoInput> = {}
  const sslConfigs: Record<string, SslConfig> = {}
  const registries: Record<string, string> = {}
  for (const [configKey, value] of Object.entries(rawConfig)) {
    if (configKey[0] === '@' && configKey.endsWith(':registry')) {
      registries[configKey.slice(0, configKey.indexOf(':'))] = normalizeRegistryUrl(value as string)
      continue
    }

    const parsed = tryParseAuthSetting(configKey) ?? tryParseSslSetting(configKey)

    switch (parsed?.target) {
    case undefined:
      continue
    case 'auth': {
      const { authInputKey, registry } = parsed
      authInfoInputs[registry] ??= {}
      authInfoInputs[registry][authInputKey] = value as string
      continue
    }
    case 'ssl': {
      const { registry, sslConfigKey, isFile } = parsed
      sslConfigs[registry] ??= { cert: '', key: '' }
      sslConfigs[registry][sslConfigKey] = isFile
        ? fs.readFileSync(value as string, 'utf8')
        : (value as string).replace(/\\n/g, '\n')
      continue
    }
    default: {
      const _typeGuard: never = parsed
      throw new Error(`Unhandled variant: ${JSON.stringify(_typeGuard)}`)
    }
    }
  }

  // Instead of directly returning the object literal at the end of the function,
  // we create a temporary object of `networkConfigs` to avoid adding
  // `authInfos: undefined` to the returning object to prevent the failures of
  // existing tests which use `expect().to[Strict]Equal()` methods.
  const networkConfigs: NetworkConfigs = {
    registries,
    sslConfigs,
  }

  for (const key in authInfoInputs) {
    const authInfo = parseAuthInfo(authInfoInputs[key])
    if (authInfo) {
      networkConfigs.authInfos ??= {}
      networkConfigs.authInfos[key] = authInfo
    }
  }

  return networkConfigs
}

export function getDefaultAuthInfo (rawConfig: Record<string, unknown>): AuthInfo | undefined {
  const input: AuthInfoInput = {}
  for (const rawKey in AUTH_SUFFIX_KEY_MAP) {
    const key = AUTH_SUFFIX_KEY_MAP[rawKey]
    const value = rawConfig[rawKey] as string | undefined
    if (value != null) {
      input[key] = value
    }
  }
  return parseAuthInfo(input)
}

const AUTH_SUFFIX_RE = /:(?<key>_auth|_authToken|_password|username|tokenHelper)$/
const AUTH_SUFFIX_KEY_MAP: Record<string, keyof AuthInfoInput> = {
  _auth: 'authPairBase64',
  _authToken: 'authToken',
  _password: 'authPassword',
  username: 'authUsername',
  tokenHelper: 'tokenHelper',
}

interface ParsedAuthSetting {
  target: 'auth'
  registry: string
  authInputKey: keyof AuthInfoInput
}

function tryParseAuthSetting (key: string): ParsedAuthSetting | undefined {
  const match = key.match(AUTH_SUFFIX_RE)
  if (!match?.groups) {
    return undefined
  }
  const registry = key.slice(0, match.index!) // already includes the trailing slash
  const authInputKey = AUTH_SUFFIX_KEY_MAP[match.groups.key]
  if (!authInputKey) {
    throw new Error(`Unexpected key: ${match.groups.key}`)
  }
  return { target: 'auth', registry, authInputKey }
}

const SSL_SUFFIX_RE = /:(?<id>cert|key|ca)(?<kind>file)?$/

interface ParsedSslSetting {
  target: 'ssl'
  registry: string
  sslConfigKey: keyof SslConfig
  isFile: boolean
}

function tryParseSslSetting (key: string): ParsedSslSetting | undefined {
  const match = key.match(SSL_SUFFIX_RE)
  if (!match?.groups) {
    return undefined
  }
  const registry = key.slice(0, match.index!) // already includes the trailing slash
  const sslConfigKey = match.groups.id as keyof SslConfig
  const isFile = Boolean(match.groups.kind)
  return { target: 'ssl', registry, sslConfigKey, isFile }
}
