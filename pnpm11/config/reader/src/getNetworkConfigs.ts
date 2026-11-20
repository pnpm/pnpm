import fs from 'node:fs'

import { type Creds, DEFAULT_REGISTRY_SCOPE, type RegistryConfig } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'

import { parseCreds, type RawCreds } from './parseCreds.js'

export interface NetworkConfigs {
  configByUri?: Record<string, RegistryConfig> // TODO: remove optional from here, this means that tests would have to be updated.
  registries: Record<string, string>
}

export function getNetworkConfigs (rawConfig: Record<string, unknown>): NetworkConfigs {
  const collected: CollectedNetworkConfigs = {
    rawCredsMap: {},
    networkConfigs: { registries: {} },
  }
  for (const [configKey, value] of Object.entries(rawConfig)) {
    collectNetworkConfigEntry(collected, configKey, value)
  }
  addParsedCredsToConfigByUri(collected)
  return collected.networkConfigs
}

interface CollectedNetworkConfigs {
  rawCredsMap: Record<string, Record<string, RawCreds>>
  networkConfigs: NetworkConfigs
}

function collectNetworkConfigEntry ({ rawCredsMap, networkConfigs }: CollectedNetworkConfigs, configKey: string, value: unknown): void {
  if (configKey[0] === '@' && configKey.endsWith(':registry')) {
    networkConfigs.registries[configKey.slice(0, configKey.indexOf(':'))] = normalizeRegistryUrl(value as string)
    return
  }

  const parsedCreds = tryParseCredsKey(configKey)
  if (parsedCreds) {
    const { credsField, registry, scope } = parsedCreds
    rawCredsMap[registry] ??= {}
    rawCredsMap[registry][scope ?? DEFAULT_REGISTRY_SCOPE] ??= {}
    rawCredsMap[registry][scope ?? DEFAULT_REGISTRY_SCOPE][credsField] = value as string
    return
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

function addParsedCredsToConfigByUri ({ rawCredsMap, networkConfigs }: CollectedNetworkConfigs): void {
  for (const uri in rawCredsMap) {
    const scopedCreds = getScopedCreds(rawCredsMap[uri])
    if (Object.keys(scopedCreds).length > 0) {
      networkConfigs.configByUri ??= {}
      networkConfigs.configByUri[uri] ??= {}
      Object.assign(networkConfigs.configByUri[uri], scopedCreds)
    }
  }
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
  scope?: string
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
  return { ...splitScopeFromRegistry(registry), credsField }
}

function getScopedCreds (rawCredsByScope: Record<string, RawCreds> = {}): Record<string, Creds> {
  const scopedCreds: Record<string, Creds> = {}
  for (const [scope, rawCreds] of Object.entries(rawCredsByScope)) {
    const creds = parseCreds(rawCreds)
    if (creds) {
      scopedCreds[scope] = creds
    }
  }
  return scopedCreds
}

function splitScopeFromRegistry (registry: string): { registry: string, scope?: string } {
  const colonScope = splitScopeFromRegistryByColon(registry)
  if (colonScope) return colonScope
  return splitScopeFromRegistryByPath(registry)
}

function splitScopeFromRegistryByColon (registry: string): { registry: string, scope: string } | undefined {
  if (!registry.startsWith('//')) return undefined
  const scopeSeparatorIndex = registry.lastIndexOf(':@')
  if (scopeSeparatorIndex === -1) return undefined
  const scope = registry.slice(scopeSeparatorIndex + 1)
  if (!isPackageScope(scope)) return undefined
  return {
    registry: normalizeRegistryKey(registry.slice(0, scopeSeparatorIndex)),
    scope,
  }
}

function splitScopeFromRegistryByPath (registry: string): { registry: string, scope?: string } {
  if (!registry.startsWith('//')) return { registry }
  const trimmed = registry.endsWith('/') ? registry.slice(0, -1) : registry
  const lastSlashIndex = trimmed.lastIndexOf('/')
  if (lastSlashIndex === -1) return { registry }
  const scope = trimmed.slice(lastSlashIndex + 1)
  if (!isPackageScope(scope)) return { registry }
  return {
    registry: trimmed.slice(0, lastSlashIndex + 1),
    scope,
  }
}

function isPackageScope (scope: string): boolean {
  return scope.startsWith('@') && scope.length > 1 && !scope.includes('/') && !scope.includes(':')
}

function normalizeRegistryKey (registry: string): string {
  return registry.endsWith('/') ? registry : `${registry}/`
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
