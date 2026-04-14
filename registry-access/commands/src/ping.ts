import util from 'node:util'

import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'

import { rcOptionsTypes as commonRcOptionsTypes } from './common.js'

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...commonRcOptionsTypes(),
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return commonRcOptionsTypes()
}

export interface PingOptions extends CreateFetchFromRegistryOptions {
  registry?: string
  registries?: Registries
  configByUri?: Record<string, RegistryConfig>
}

export const commandNames = ['ping']

export function help (): string {
  return renderHelp({
    description: 'Test connectivity to the configured registry.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Test a specific registry URL',
            name: '--registry <url>',
          },
        ],
      },
    ],
    url: docsUrl('ping'),
    usages: ['pnpm ping [--registry <url>]'],
  })
}

export async function handler (opts: PingOptions): Promise<string> {
  const registryUrl = opts.registry ?? opts.registries?.default ?? 'https://registry.npmjs.org/'
  const normalizedRegistryUrl = registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
  const pingUrlObject = new URL('./-/ping', normalizedRegistryUrl)
  pingUrlObject.searchParams.set('write', 'true')
  const pingUrl = pingUrlObject.toString()

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, normalizedRegistryUrl)
  const authHeaderValue = getAuthHeader(normalizedRegistryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const start = Date.now()
  let response
  try {
    response = await fetchFromRegistry(pingUrl, {
      retry: { retries: 0 },
      authHeaderValue,
    })
  } catch (err: unknown) {
    const errorMessage = util.types.isNativeError(err) ? err.message : String(err)
    throw new PnpmError('PING_ERROR', `Failed to reach registry: ${errorMessage}`)
  }

  if (!response.ok) {
    throw new PnpmError(
      'PING_ERROR',
      `Failed to reach registry: ${response.status} ${response.statusText}`.trimEnd()
    )
  }

  const body = await response.text()
  const time = Date.now() - start

  let details = ''
  if (body) {
    try {
      const parsed = JSON.parse(body)
      if (parsed && typeof parsed === 'object' && Object.keys(parsed).length > 0) {
        details = JSON.stringify(parsed, null, 2)
      }
    } catch {
      // non-JSON body — ignore
    }
  }

  const lines = [`PING ${registryUrl}`, `PONG ${time}ms`]
  if (details) lines.push(`PONG ${details}`)
  return lines.join('\n')
}
