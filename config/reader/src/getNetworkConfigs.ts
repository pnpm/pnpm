import fs from 'node:fs'

import type { Creds } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'

import { parseCreds, type RawCreds } from './parseCreds.js'

export interface NetworkConfigs {
  credsByUri?: Record<string, Creds> // TODO: remove optional from here, this means that tests would have to be updated.
  registries: Record<string, string>
}

export function getNetworkConfigs (rawConfig: Record<string, unknown>): NetworkConfigs {
  const rawCredsMap: Record<string, RawCreds> = {}
  const sslByUri: Record<string, Partial<Pick<Creds, 'cert' | 'key' | 'ca'>>> = {}
  const registries: Record<string, string> = {}
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
      sslByUri[registry] ??= {}
      sslByUri[registry][sslField] = isFile
        ? fs.readFileSync(value as string, 'utf8')
        : (value as string).replace(/\\n/g, '\n')
    }
  }

  // Instead of directly returning the object literal at the end of the function,
  // we create a temporary object of `networkConfigs` to avoid adding
  // `credsByUri: undefined` to the returning object to prevent the failures of
  // existing tests which use `expect().to[Strict]Equal()` methods.
  const networkConfigs: NetworkConfigs = {
    registries,
  }

  // Collect all registry URIs that have either auth or SSL config
  const allUris = new Set([...Object.keys(rawCredsMap), ...Object.keys(sslByUri)])
  for (const uri of allUris) {
    const parsedAuth = rawCredsMap[uri] ? parseCreds(rawCredsMap[uri]) : undefined
    const ssl = sslByUri[uri]
    if (parsedAuth || ssl) {
      networkConfigs.credsByUri ??= {}
      networkConfigs.credsByUri[uri] = { ...parsedAuth, ...ssl }
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
