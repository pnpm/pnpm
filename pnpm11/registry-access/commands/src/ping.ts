
import { docsUrl } from '@pnpm/cli.utils'
import { isError, PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import type { RegistryConfig } from '@pnpm/types'
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
  const registryUrl = opts.registry ?? 'https://registry.npmjs.org/'
  const normalizedRegistryUrl = registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
  const pingUrl = buildPingUrl(normalizedRegistryUrl)

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
  const authHeaderValue = getAuthHeader(normalizedRegistryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const start = Date.now()
  const response = await sendPing(fetchFromRegistry, { pingUrl, authHeaderValue })
  const body = await response.text()
  const time = Date.now() - start

  const details = formatPingDetails(body)
  const lines = [`PING ${registryUrl}`, `PONG ${time}ms`]
  if (details) lines.push(`PONG ${details}`)
  return lines.join('\n')
}

async function sendPing (
  fetchFromRegistry: FetchFromRegistry,
  { pingUrl, authHeaderValue }: { pingUrl: string, authHeaderValue: string | undefined }
): Promise<Response> {
  let response
  try {
    response = await fetchFromRegistry(pingUrl, {
      retry: { retries: 0 },
      authHeaderValue,
    })
  } catch (err: unknown) {
    const errorMessage = isError(err) ? err.message : String(err)
    throw new PnpmError('PING_ERROR', `Failed to reach registry: ${errorMessage}`)
  }

  if (!response.ok) {
    throw new PnpmError(
      'PING_ERROR',
      `Failed to reach registry: ${response.status} ${response.statusText}`.trimEnd()
    )
  }
  return response
}

function buildPingUrl (normalizedRegistryUrl: string): string {
  const pingUrlObject = new URL('./-/ping', normalizedRegistryUrl)
  pingUrlObject.searchParams.set('write', 'true')
  return pingUrlObject.toString()
}

function formatPingDetails (body: string): string {
  if (!body) return ''
  let parsed
  try {
    parsed = JSON.parse(body)
  } catch {
    // non-JSON body — ignore
    return ''
  }
  if (parsed && typeof parsed === 'object' && Object.keys(parsed).length > 0) {
    return JSON.stringify(parsed, null, 2)
  }
  return ''
}
