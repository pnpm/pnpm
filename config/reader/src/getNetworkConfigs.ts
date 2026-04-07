import fs from 'node:fs'

import type { Creds, RegistryConfig } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'

import { parseCreds, type RawCreds } from './parseCreds.js'

export interface NetworkConfigs {
  configByUri?: Record<string, RegistryConfig> // TODO: remove optional from here, this means that tests would have to be updated.
  registries: Record<string, string>
}

export function getNetworkConfigs (rawConfig: Record<string, unknown>): NetworkConfigs {
  const rawCredsMap: Record<string, RawCreds> = {}
  const registries: Record<string, string> = {}
  const networkConfigs: NetworkConfigs = { registries }
  for (const [configKey, value] of Object.entries(rawConfig)) {
    if (configKey[0] === '@' && configKey.endsWith(':registry')) {
      registries[configKey.slice(0, configKey.indexOf(':'))] = normalizeRegistryUrl(value as string)
      continue
    }

    const parsedCreds = tryParseCredsKey(configKey)
    if (parsedCreds) {
      const { credsField, registry } = parsedCreds
      rawCredsMap[registry] ??= {}
      rawCredsMap[registry][credsField] = value as string
      continue
    }

    const parsedSsl = tryParseSslKey(configKey)
    if (parsedSsl) {
      const { registry, sslField, isFile } = parsedSsl
      networkConfigs.configByUri ??= {}
      networkConfigs.configByUri[registry] ??= {}
      networkConfigs.configByUri[registry].tls ??= {}
      networkConfigs.configByUri[registry].tls[sslField] = isFile
        ? fs.readFileSync(value as string, 'utf8')
        : (value as string).replace(/\\n/g, '\n')
    }
  }

  for (const uri in rawCredsMap) {
    const creds = parseCreds(rawCredsMap[uri])
    if (creds) {
      networkConfigs.configByUri ??= {}
      networkConfigs.configByUri[uri] ??= {}
      networkConfigs.configByUri[uri].creds = creds
    }
  }

  return networkConfigs
}

export function getDefaultCreds (rawConfig: Record<string, unknown>): Creds | undefined {
  const input: RawCreds = {}
  for (const rawKey in AUTH_SUFFIX_KEY_MAP) {
    const key = AUTH_SUFFIX_KEY_MAP[rawKey]
    const value = rawConfig[rawKey] as string | undefined
    if (value != null) {
      input[key] = value
    }
  }
  return parseCreds(input)
}

const AUTH_SUFFIX_RE = /:(?<key>_auth|_authToken|_password|username|tokenHelper)$/
const AUTH_SUFFIX_KEY_MAP: Record<string, keyof RawCreds> = {
  _auth: 'authPairBase64',
  _authToken: 'authToken',
  _password: 'authPassword',
  username: 'authUsername',
  tokenHelper: 'tokenHelper',
}

interface ParsedCredsKey {
  registry: string
  credsField: keyof RawCreds
}

function tryParseCredsKey (key: string): ParsedCredsKey | undefined {
  const match = key.match(AUTH_SUFFIX_RE)
  if (!match?.groups) {
    return undefined
  }
  const registry = key.slice(0, match.index!) // already includes the trailing slash
  const credsField = AUTH_SUFFIX_KEY_MAP[match.groups.key]
  if (!credsField) {
    throw new Error(`Unexpected key: ${match.groups.key}`)
  }
  return { registry, credsField }
}

const SSL_SUFFIX_RE = /:(?<id>cert|key|ca)(?<kind>file)?$/

type SslField = 'cert' | 'key' | 'ca'

interface ParsedSslKey {
  registry: string
  sslField: SslField
  isFile: boolean
}

function tryParseSslKey (key: string): ParsedSslKey | undefined {
  const match = key.match(SSL_SUFFIX_RE)
  if (!match?.groups) {
    return undefined
  }
  const registry = key.slice(0, match.index!) // already includes the trailing slash
  const sslField = match.groups.id as SslField
  const isFile = Boolean(match.groups.kind)
  return { registry, sslField, isFile }
}
