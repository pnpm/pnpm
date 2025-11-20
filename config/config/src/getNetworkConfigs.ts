import type { SslConfig } from '@pnpm/types'
import fs from 'fs'
import normalizeRegistryUrl from 'normalize-registry-url'

export interface GetNetworkConfigsResult {
  sslConfigs: Record<string, SslConfig>
  registries: Record<string, string>
}

const validKeys = ['cert', 'key', 'ca'] as const

export function getNetworkConfigs (rawConfig: Record<string, object>): GetNetworkConfigsResult {
  // Get all the auth options that have SSL certificate data or file references
  const sslConfigs: Record<string, SslConfig> = {}
  const registries: Record<string, string> = {}
  for (const [configKey, value] of Object.entries(rawConfig)) {
    // exact matches
    const isFileReference = validKeys.some(k => configKey.endsWith(`:${k}file`))
    const isCertData = validKeys.some(k => configKey.endsWith(`:${k}`))

    if (configKey[0] === '@' && configKey.endsWith(':registry')) {
      registries[configKey.slice(0, configKey.indexOf(':'))] = normalizeRegistryUrl(value as unknown as string)
    } else if (isFileReference || isCertData) {
      // Split by '/:' because the registry may contain a port
      const registry = configKey.split('/:')[0] + '/'
      if (!sslConfigs[registry]) {
        sslConfigs[registry] = { cert: '', key: '' }
      }
      for (const key of validKeys) {
        if (configKey.includes(`:${key}file`)) {
          sslConfigs[registry][key] = fs.readFileSync(value as unknown as string, 'utf8')
        } else if (configKey.includes(`:${key}`)) {
          sslConfigs[registry][key] = (value as unknown as string).replace(/\\n/g, '\n')
        }
      }
    }
  }
  return {
    registries,
    sslConfigs,
  }
}
