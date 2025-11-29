import fs from 'fs'
import { type SslConfig } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'

export interface GetNetworkConfigsResult {
  sslConfigs: Record<string, SslConfig>
  registries: Record<string, string>
}

export function getNetworkConfigs (rawConfig: Record<string, object>): GetNetworkConfigsResult {
  // Get all the auth options that have SSL certificate data or file references
  const sslConfigs: Record<string, SslConfig> = {}
  const registries: Record<string, string> = {}
  for (const [configKey, value] of Object.entries(rawConfig)) {
    if (configKey[0] === '@' && configKey.endsWith(':registry')) {
      registries[configKey.slice(0, configKey.indexOf(':'))] = normalizeRegistryUrl(value as unknown as string)
      continue
    }

    const parsed = tryParseSslSetting(configKey)
    if (!parsed) continue

    const { registry, sslConfigKey, isFile } = parsed
    if (!sslConfigs[registry]) {
      sslConfigs[registry] = { cert: '', key: '' }
    }
    sslConfigs[registry][sslConfigKey] = isFile
      ? fs.readFileSync(value as unknown as string, 'utf8')
      : (value as unknown as string).replace(/\\n/g, '\n')
  }
  return {
    registries,
    sslConfigs,
  }
}

const SSL_SUFFIX_RE = /:(?<id>cert|key|ca)(?<kind>file)?$/

interface ParsedSslSetting {
  registry: string
  sslConfigKey: keyof SslConfig
  isFile: boolean
}

function tryParseSslSetting (key: string): ParsedSslSetting | null {
  const match = key.match(SSL_SUFFIX_RE)
  if (!match?.groups) {
    return null
  }
  const registry = key.slice(0, match.index!) // already includes the trailing slash
  const sslConfigKey = match.groups.id as keyof SslConfig
  const isFile = Boolean(match.groups.kind)
  return { registry, sslConfigKey, isFile }
}
